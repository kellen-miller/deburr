package analysis

import (
	"context"
	"fmt"
	"go/ast"
	"go/parser"
	"go/token"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/kellen-miller/deburr/internal/report"
)

type FileFailure struct {
	Path    string
	Status  report.FileStatus
	Message string
}

// AnalysisError reports source failures after the partial report has been
// assembled. Callers can still render the report while returning this error.
type AnalysisError struct {
	Failures []FileFailure
	Errors   []string
}

func (e *AnalysisError) Error() string {
	if len(e.Failures) == 0 && len(e.Errors) == 0 {
		return "analysis failed"
	}

	parts := make([]string, 0, len(e.Failures))
	for _, failure := range e.Failures {
		parts = append(parts, fmt.Sprintf("%s: %s", failure.Path, failure.Message))
	}

	parts = append(parts, e.Errors...)
	return "analysis failed: " + strings.Join(parts, "; ")
}

type sourceFile struct {
	source      []byte
	path        string
	category    report.Category
	status      report.FileStatus
	reason      string
	err         string
	fileSet     *token.FileSet
	astFile     *ast.File
	lineCode    []bool
	bytes       int64
	lines       int
	codeLines   int
	tsFunctions []report.Function
	tsTokens    []typescriptSourceToken
}

type analysisState struct {
	progress  io.Writer
	root      string
	base      string
	manifest  string
	files     []*sourceFile
	failures  []FileFailure
	errors    []string
	functions []report.Function
	findings  []report.Finding
	cfg       Config
	report    report.Report
}

// Analyze reads and parses Go and TypeScript source under root.
// No target code or build hooks are executed. Progress is best effort; use
// io.Discard to suppress it.
func Analyze(ctx context.Context, root string, cfg Config, progress io.Writer) (report.Report, error) {
	cfg = cfg.normalized()
	result := report.Report{
		SchemaVersion: report.SchemaVersion,
		Analyzer: report.AnalyzerIdentity{
			ID:      AnalyzerID,
			Version: AnalyzerVersion,
		},
		Config:      cfg.reportSummary(),
		Scope:       cfg.reportScope(),
		Duplication: duplicationResult(cfg),
		Files:       []report.File{},
		Functions:   []report.Function{},
		Findings:    []report.Finding{},
	}

	input, err := filepath.Abs(filepath.Clean(root))
	if err != nil {
		return result, fmt.Errorf("resolve input: %w", err)
	}

	info, err := os.Lstat(input)
	if err != nil {
		return result, fmt.Errorf("stat input: %w", err)
	}

	state := &analysisState{root: input, cfg: cfg, report: result, progress: progress}
	// Progress is best effort and separate from the report's output stream.
	state.writeProgressf("deburr: scanning Go and TypeScript source\n")
	if info.Mode()&os.ModeSymlink != 0 {
		state.base = filepath.Dir(input)
		state.visitSymlink(filepath.Base(input), info)
	} else if info.IsDir() {
		state.base = input
		err = state.walk(ctx)
	} else {
		state.base = filepath.Dir(input)
		state.visit(ctx, input, nil)
	}

	if err != nil {
		state.failures = append(state.failures, FileFailure{
			Path:    ".",
			Status:  report.FileReadError,
			Message: cleanError(err),
		})
	}

	state.writeProgressf("deburr: scan finished: %d files, %d analyzed, %d errors\n",
		state.report.Coverage.Discovered, state.report.Coverage.Analyzed,
		state.report.Coverage.ReadErrors+state.report.Coverage.ParseErrors)
	state.writeProgressf("deburr: summarizing metrics\n")
	state.finish()
	if cfg.Duplication.Requested {
		duplication, duplicationErr := runDuplication(ctx, state)
		state.report.Duplication = duplication
		if duplicationErr != nil {
			state.errors = append(state.errors, duplicationErr.Error())
		}
	}

	if len(state.failures) > 0 || len(state.errors) > 0 {
		return state.report, &AnalysisError{Failures: state.failures, Errors: state.errors}
	}

	return state.report, nil
}

func (s *analysisState) writeProgressf(format string, args ...any) {
	if _, err := fmt.Fprintf(s.progress, format, args...); err != nil {
		s.progress = io.Discard
	}
}

func duplicationResult(cfg Config) *report.Duplication {
	config := cfg.reportSummary().Duplication
	if !cfg.Duplication.Requested {
		return &report.Duplication{Status: report.DuplicationNotRequested, Config: config}
	}

	return &report.Duplication{
		Status: report.DuplicationError,
		Config: config,
		Error:  "duplication adapter is not enabled in the native Go and TypeScript analyzer",
	}
}

func (s *analysisState) walk(ctx context.Context) error {
	err := filepath.WalkDir(s.root, func(path string, entry os.DirEntry, walkErr error) error {
		if err := ctx.Err(); err != nil {
			return fmt.Errorf("walk canceled: %w", err)
		}

		if walkErr != nil {
			rel := s.relative(path)
			s.recordFailure(rel, cleanError(walkErr))
			return nil
		}

		if path == s.root {
			return nil
		}

		rel := s.relative(path)
		if entry.IsDir() {
			if skipDirectory(rel) {
				s.report.Coverage.ExcludedDirectories++
				return filepath.SkipDir
			}

			return nil
		}

		if entry.Type()&os.ModeSymlink != 0 {
			info, infoErr := entry.Info()
			if infoErr == nil {
				s.visitSymlink(rel, info)
			} else {
				s.recordFailure(rel, cleanError(infoErr))
			}

			return nil
		}

		s.visit(ctx, path, entry)
		return nil
	})
	if err != nil {
		return fmt.Errorf("walk source: %w", err)
	}

	return nil
}

func (s *analysisState) visitSymlink(path string, info os.FileInfo) {
	bytes := int64(0)
	if info != nil {
		bytes = info.Size()
	}

	s.recordFile(&sourceFile{
		path:     slashPath(path),
		category: categoryForPath(path),
		status:   report.FileExcluded,
		bytes:    bytes,
		reason:   "symlink not followed",
	})
	s.report.Coverage.Symlinks++
}

func (s *analysisState) visit(ctx context.Context, path string, entry os.DirEntry) {
	rel := s.relative(path)
	if err := ctx.Err(); err != nil {
		s.recordFailure(rel, cleanError(err))
		return
	}

	file := s.discoverFile(path, rel, entry)
	if file == nil {
		return
	}

	if file.status == report.FileAnalyzed {
		s.parseFile(file)
	} else if isTypeScriptPath(file.path) {
		// Exclusion owns the status; valid syntax still contributes coverage
		// counts, while invalid excluded syntax has unknown code-line counts.
		tree, language, err := parseTypeScript(file.path, file.source)
		if err == nil {
			file.lineCode = typescriptLineMap(tree.RootNode(), language, file.source)
			for _, code := range file.lineCode {
				if code {
					file.codeLines++
				}
			}

			tree.Release()
		}
	}

	s.recordFile(file)
}

func (s *analysisState) discoverFile(path, rel string, entry os.DirEntry) *sourceFile {
	info, err := os.Lstat(path)
	if entry != nil {
		info, err = entry.Info()
	}

	if err != nil {
		s.recordFailure(rel, cleanError(err))
		return nil
	}

	if !info.Mode().IsRegular() {
		return nil
	}

	if !supportedSourcePath(info.Name()) {
		s.recordFile(&sourceFile{
			path:     rel,
			category: categoryForPath(rel),
			status:   report.FileUnsupported,
			bytes:    info.Size(),
			reason:   "language is not supported",
		})
		return nil
	}

	if s.cfg.MaxFileBytes > 0 && info.Size() > s.cfg.MaxFileBytes {
		s.recordFile(&sourceFile{
			path:     rel,
			category: categoryForPath(rel),
			status:   report.FileExcluded,
			bytes:    info.Size(),
			reason:   "file exceeds max_file_bytes",
		})
		return nil
	}

	source, err := os.ReadFile(path)
	if err != nil {
		s.recordFailure(rel, cleanError(err))
		return nil
	}

	var lineCode []bool
	if !isTypeScriptPath(rel) {
		lineCode = sourceLineMap(rel, source)
	}

	lines := physicalLineCount(source)
	codeLines := 0
	for _, code := range lineCode {
		if code {
			codeLines++
		}
	}

	category := categoryForPath(rel)
	if category != report.CategoryVendor && isGenerated(rel, source) {
		category = report.CategoryGenerated
	}

	file := &sourceFile{
		path:      rel,
		category:  category,
		status:    report.FileAnalyzed,
		bytes:     int64(len(source)),
		lines:     lines,
		codeLines: codeLines,
		source:    source,
		lineCode:  lineCode,
	}

	if reason := excludedCategoryReason(category, s.cfg); reason != "" {
		file.status = report.FileExcluded
		file.reason = reason
	}

	return file
}

func excludedCategoryReason(category report.Category, config Config) string {
	switch category {
	case report.CategoryVendor:
		if !config.IncludeVendor {
			return "vendor excluded by configuration"
		}
	case report.CategoryGenerated:
		if !config.IncludeGenerated {
			return "generated excluded by configuration"
		}
	case report.CategoryTest:
		if config.ExcludeTests {
			return "tests excluded by configuration"
		}
	case report.CategoryTestdata:
		if !config.IncludeTestdata {
			return "testdata excluded by configuration"
		}
	case report.CategoryProduction, report.CategoryUnsupported:
	}

	return ""
}

func (s *analysisState) parseFile(file *sourceFile) {
	if isTypeScriptPath(file.path) {
		tree, language, err := parseTypeScript(file.path, file.source)
		if err != nil {
			file.status = report.FileParseError
			file.err = cleanError(err)
			s.failures = append(s.failures, FileFailure{Path: file.path, Status: file.status, Message: file.err})
			return
		}

		defer tree.Release()
		root := tree.RootNode()
		file.lineCode = typescriptLineMap(root, language, file.source)
		for _, code := range file.lineCode {
			if code {
				file.codeLines++
			}
		}

		functions, findings := analyzeTypeScript(file, root, language)
		file.tsFunctions = functions
		file.tsTokens = typescriptTokens(root, language, file.source)
		s.functions = append(s.functions, functions...)
		s.findings = append(s.findings, findings...)
		return
	}

	fset := token.NewFileSet()
	parsed, parseErr := parser.ParseFile(fset, file.path, file.source, parser.ParseComments|parser.AllErrors)
	if parseErr != nil {
		file.status = report.FileParseError
		file.err = cleanError(parseErr)
		s.failures = append(s.failures, FileFailure{
			Path:    file.path,
			Status:  report.FileParseError,
			Message: file.err,
		})
		return
	}

	file.fileSet = fset
	file.astFile = parsed
	functions, findings := analyzeSource(file)
	s.functions = append(s.functions, functions...)
	s.findings = append(s.findings, findings...)
}

func (s *analysisState) recordFailure(path string, message string) {
	category := report.CategoryUnsupported
	if supportedSourcePath(path) {
		category = categoryForPath(path)
	}

	s.recordFile(&sourceFile{
		path:     slashPath(path),
		category: category,
		status:   report.FileReadError,
		err:      message,
		reason:   "source could not be analyzed",
	})
	s.failures = append(s.failures, FileFailure{
		Path:    slashPath(path),
		Status:  report.FileReadError,
		Message: message,
	})
}

func (s *analysisState) recordFile(file *sourceFile) {
	file.path = slashPath(file.path)
	s.files = append(s.files, file)
	s.report.Coverage.Discovered++
	switch file.status {
	case report.FileAnalyzed:
		s.report.Coverage.Analyzed++
	case report.FileExcluded:
		s.report.Coverage.Excluded++
	case report.FileUnsupported:
		s.report.Coverage.Unsupported++
	case report.FileReadError:
		s.report.Coverage.ReadErrors++
	case report.FileParseError:
		s.report.Coverage.ParseErrors++
	}

	if s.report.Coverage.Discovered == 1 || s.report.Coverage.Discovered%100 == 0 {
		s.writeProgressf(
			"deburr: scanned %d files (%d analyzed, %d excluded, %d unsupported, %d errors)\n",
			s.report.Coverage.Discovered,
			s.report.Coverage.Analyzed,
			s.report.Coverage.Excluded,
			s.report.Coverage.Unsupported,
			s.report.Coverage.ReadErrors+s.report.Coverage.ParseErrors,
		)
	}
}

func (s *analysisState) finish() {
	sort.Slice(s.files, func(i, j int) bool { return s.files[i].path < s.files[j].path })
	s.manifest = sourceManifestDigest(s.files)
	s.report.Provenance.Source.ManifestDigest = s.manifest
	s.report.Files, s.report.Coverage.ByCategory = buildFileReports(s.files)

	sort.Slice(s.functions, func(i, j int) bool {
		left, right := s.functions[i], s.functions[j]
		if left.Path != right.Path {
			return left.Path < right.Path
		}

		if left.Start.Line != right.Start.Line {
			return left.Start.Line < right.Start.Line
		}

		if left.Start.Column != right.Start.Column {
			return left.Start.Column < right.Start.Column
		}

		if left.Name != right.Name {
			return left.Name < right.Name
		}

		return left.ID < right.ID
	})
	s.report.Functions = append([]report.Function(nil), s.functions...)

	sort.Slice(s.findings, func(i, j int) bool {
		left, right := s.findings[i], s.findings[j]
		if left.Path != right.Path {
			return left.Path < right.Path
		}

		if left.Start.Line != right.Start.Line {
			return left.Start.Line < right.Start.Line
		}

		if left.Start.Column != right.Start.Column {
			return left.Start.Column < right.Start.Column
		}

		if left.Rule != right.Rule {
			return left.Rule < right.Rule
		}

		return left.ID < right.ID
	})
	s.report.Findings = append([]report.Finding(nil), s.findings...)
	s.report.Metrics = calculateMetrics(s.report.Files, s.report.Functions)
}
