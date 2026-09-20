package analysis

import (
	"context"
	"crypto/sha256"
	"fmt"
	"go/ast"
	"go/parser"
	"go/token"
	"os"
	"path/filepath"
	"sort"
	"strconv"
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
	source    []byte
	path      string
	category  report.Category
	status    report.FileStatus
	reason    string
	err       string
	fileSet   *token.FileSet
	astFile   *ast.File
	lineCode  []bool
	bytes     int64
	lines     int
	codeLines int
}

type analysisState struct {
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

// Analyze scans root with the native Go analyzer. The target source is read
// and parsed only; no target code or build hooks are executed.
func Analyze(ctx context.Context, root string, cfg Config) (report.Report, error) {
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

	state := &analysisState{root: input, cfg: cfg, report: result}
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

func duplicationResult(cfg Config) *report.Duplication {
	config := cfg.reportSummary().Duplication
	if !cfg.Duplication.Requested {
		return &report.Duplication{Status: report.DuplicationNotRequested, Config: config}
	}

	return &report.Duplication{
		Status: report.DuplicationError,
		Config: config,
		Error:  "duplication adapter is not enabled in the native Go analyzer",
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

	if !strings.HasSuffix(info.Name(), ".go") {
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

	lines, codeLines := sourceLineCounts(rel, source)
	category := categoryForPath(rel)
	if category != report.CategoryVendor && isGenerated(source) {
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
		lineCode:  sourceLineMap(rel, source),
	}

	if category == report.CategoryVendor && !s.cfg.IncludeVendor {
		file.status = report.FileExcluded
		file.reason = "vendor excluded by configuration"
	} else if category == report.CategoryGenerated && !s.cfg.IncludeGenerated {
		file.status = report.FileExcluded
		file.reason = "generated excluded by configuration"
	} else if category == report.CategoryTest && s.cfg.ExcludeTests {
		file.status = report.FileExcluded
		file.reason = "tests excluded by configuration"
	} else if category == report.CategoryTestdata && !s.cfg.IncludeTestdata {
		file.status = report.FileExcluded
		file.reason = "testdata excluded by configuration"
	}

	return file
}

func (s *analysisState) parseFile(file *sourceFile) {
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
	if strings.HasSuffix(path, ".go") {
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

func sourceManifestDigest(files []*sourceFile) string {
	ordered := append([]*sourceFile(nil), files...)
	sort.SliceStable(ordered, func(left, right int) bool {
		return ordered[left].path < ordered[right].path
	})

	hash := sha256.New()
	for _, file := range ordered {
		path := slashPath(file.path)
		frame := strconv.Itoa(len(path)) + ":" + path + ":" + strconv.Itoa(len(file.source)) + ":"
		_, _ = hash.Write([]byte(frame))
		_, _ = hash.Write(file.source)
		_, _ = hash.Write([]byte{'\x00'})
	}

	return fmt.Sprintf("sha256:%x", hash.Sum(nil))
}

func buildFileReports(files []*sourceFile) ([]report.File, []report.CategoryCoverage) {
	reports := make([]report.File, 0, len(files))
	categoryCounts := make(map[report.Category]*report.CategoryCoverage, len(coverageCategories()))
	for _, category := range coverageCategories() {
		categoryCounts[category] = &report.CategoryCoverage{Category: category}
	}

	for index := range files {
		file := files[index]
		reports = append(reports, report.File{
			Path:      file.path,
			Category:  file.category,
			Status:    file.status,
			Bytes:     file.bytes,
			Lines:     file.lines,
			CodeLines: file.codeLines,
			Reason:    file.reason,
			Error:     file.err,
		})

		category := file.category
		if category == "" {
			category = report.CategoryUnsupported
		}

		counts := categoryCounts[category]
		if counts == nil {
			counts = &report.CategoryCoverage{Category: category}
			categoryCounts[category] = counts
		}

		countFileCategory(counts, file.status)
	}

	byCategory := make([]report.CategoryCoverage, 0, len(categoryCounts))
	for _, category := range coverageCategories() {
		byCategory = append(byCategory, *categoryCounts[category])
	}

	return reports, byCategory
}

func coverageCategories() []report.Category {
	return []report.Category{
		report.CategoryProduction,
		report.CategoryTest,
		report.CategoryTestdata,
		report.CategoryGenerated,
		report.CategoryVendor,
		report.CategoryUnsupported,
	}
}

func countFileCategory(counts *report.CategoryCoverage, status report.FileStatus) {
	counts.Discovered++
	switch status {
	case report.FileAnalyzed:
		counts.Analyzed++
	case report.FileExcluded:
		counts.Excluded++
	case report.FileUnsupported:
		counts.Unsupported++
	case report.FileReadError:
		counts.ReadErrors++
	case report.FileParseError:
		counts.ParseErrors++
	}
}

func calculateMetrics(files []report.File, functions []report.Function) report.Metrics {
	metrics := report.Metrics{}
	for _, file := range files {
		switch file.Status {
		case report.FileAnalyzed:
			metrics.All.CodeLines += file.CodeLines
		default:
			continue
		}

		switch file.Category {
		case report.CategoryProduction:
			metrics.Production.CodeLines += file.CodeLines
		case report.CategoryTest:
			metrics.Test.CodeLines += file.CodeLines
		default:
			continue
		}
	}

	for index := range functions {
		function := &functions[index]
		addFunctionMetric(&metrics.All, function)
		switch function.Category {
		case report.CategoryProduction:
			addFunctionMetric(&metrics.Production, function)
		case report.CategoryTest:
			addFunctionMetric(&metrics.Test, function)
		case report.CategoryTestdata, report.CategoryGenerated, report.CategoryVendor, report.CategoryUnsupported:
		}
	}

	setMassShare(&metrics.All)
	setMassShare(&metrics.Production)
	setMassShare(&metrics.Test)
	return metrics
}

func addFunctionMetric(bucket *report.MetricBucket, function *report.Function) {
	bucket.Functions++
	bucket.Cyclomatic += function.Cyclomatic
	bucket.MaxNesting = max(bucket.MaxNesting, function.MaxNesting)
	bucket.Mass += function.Mass
	if function.HighComplexity {
		bucket.HighComplexityFunctions++
		bucket.HighComplexityMass += function.Mass
	}
}

func setMassShare(bucket *report.MetricBucket) {
	if bucket.Mass == 0 {
		bucket.HighComplexityMassShare = nil
		return
	}

	share := bucket.HighComplexityMass / bucket.Mass
	bucket.HighComplexityMassShare = &share
}
