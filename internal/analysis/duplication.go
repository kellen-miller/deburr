package analysis

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/kellen-miller/deburr/internal/report"
)

const (
	duplicationTimeout        = 2 * time.Minute
	duplicationOutputLimit    = 32 << 20
	duplicationFileLimit      = 16 << 20
	duplicationSnapshotPrefix = "deburr-duplication-"
)

var semanticVersionPattern = regexp.MustCompile(`\b\d+\.\d+\.\d+\b`)

var errDuplicationOutputLimit = errors.New("duplication command output exceeded limit")

type cpdReport struct {
	Duplicates *[]cpdDuplicate `json:"duplicates"`
}

type cpdDuplicate struct {
	// Lowercase tags decode cpd's camelCase keys case-insensitively.
	FirstFile  cpdFile `json:"firstfile"`
	SecondFile cpdFile `json:"secondfile"`
	Tokens     int     `json:"tokens"`
	Lines      int     `json:"lines"`
}

type cpdFile struct {
	Name     string   `json:"name"`
	Start    int      `json:"start"`
	End      int      `json:"end"`
	StartLoc cpdPoint `json:"startloc"`
	EndLoc   cpdPoint `json:"endloc"`
}

type cpdPoint struct {
	Line   int `json:"line"`
	Column int `json:"column"`
}

type boundedBuffer struct {
	bytes.Buffer
	limit int
}

func (b *boundedBuffer) Write(value []byte) (int, error) {
	if len(value) > b.limit-b.Len() {
		return 0, errDuplicationOutputLimit
	}

	written, err := b.Buffer.Write(value)
	if err != nil {
		return written, fmt.Errorf("write bounded command output: %w", err)
	}

	return written, nil
}

func runDuplication(parent context.Context, state *analysisState) (*report.Duplication, error) {
	config := state.report.Config.Duplication
	result := &report.Duplication{Status: report.DuplicationError, Config: config}
	toolPath, err := resolveDuplicationTool(state.cfg.Duplication.Tool)
	if err != nil {
		result.Error = err.Error()
		return result, err
	}

	if err := verifyDuplicationTool(parent, state.cfg.Duplication, toolPath); err != nil {
		result.Error = err.Error()
		return result, err
	}

	tempRoot, err := os.MkdirTemp("", duplicationSnapshotPrefix)
	if err != nil {
		result.Error = "create duplication snapshot: " + err.Error()
		return result, errors.New(result.Error)
	}

	cleanupSnapshot := func(runErr error) (*report.Duplication, error) {
		cleanupErr := os.RemoveAll(tempRoot)
		if cleanupErr != nil {
			cleanupFailure := errors.New("remove duplication snapshot: " + cleanError(cleanupErr))
			result.Status = report.DuplicationError
			if result.Error == "" {
				result.Error = cleanupFailure.Error()
			} else {
				result.Error += "; " + cleanupFailure.Error()
			}

			return result, errors.Join(runErr, cleanupFailure)
		}

		return result, runErr
	}

	configPath := filepath.Join(tempRoot, "cpd-config.json")
	if err := os.WriteFile(configPath, []byte("{}\n"), 0o600); err != nil {
		result.Error = "write duplication config: " + cleanError(err)
		return cleanupSnapshot(errors.New(result.Error))
	}

	groups := duplicationGroups(state.files)
	categories := []report.Category{report.CategoryProduction, report.CategoryTest}
	clones := make([]report.Clone, 0)
	for _, category := range categories {
		files := groups[category]
		sort.Slice(files, func(i, j int) bool { return files[i].path < files[j].path })
		categoryClones, categoryErr := runDuplicationCategory(
			parent,
			state.cfg.Duplication,
			toolPath,
			configPath,
			tempRoot,
			category,
			files,
		)
		if categoryErr != nil {
			result.Error = categoryErr.Error()
			return cleanupSnapshot(categoryErr)
		}

		clones = append(clones, categoryClones...)
	}

	sort.Slice(clones, func(i, j int) bool { return clones[i].ID < clones[j].ID })
	result.Status = report.DuplicationMeasured
	result.Clones = clones
	return cleanupSnapshot(nil)
}

func duplicationGroups(files []*sourceFile) map[report.Category][]*sourceFile {
	groups := map[report.Category][]*sourceFile{
		report.CategoryProduction:  nil,
		report.CategoryTest:        nil,
		report.CategoryTestdata:    nil,
		report.CategoryGenerated:   nil,
		report.CategoryVendor:      nil,
		report.CategoryUnsupported: nil,
	}

	for index := range files {
		file := files[index]
		if file.status != report.FileAnalyzed {
			continue
		}

		if _, ok := groups[file.category]; ok {
			groups[file.category] = append(groups[file.category], file)
		}
	}

	return groups
}

func resolveDuplicationTool(tool string) (string, error) {
	path, err := exec.LookPath(tool)
	if err != nil {
		return "", fmt.Errorf("duplication tool %q is unavailable", toolLabel(tool))
	}

	path, err = filepath.Abs(path)
	if err != nil {
		return "", fmt.Errorf("resolve duplication tool %q: %s", toolLabel(tool), cleanError(err))
	}

	return path, nil
}

func verifyDuplicationTool(parent context.Context, config DuplicationConfig, toolPath string) error {
	ctx, cancel := context.WithTimeout(parent, duplicationTimeout)
	defer cancel()
	stdout, stderr, err := runBoundedCommand(ctx, toolPath, []string{"--version"}, "")
	if err != nil {
		if errors.Is(err, context.DeadlineExceeded) || errors.Is(ctx.Err(), context.DeadlineExceeded) {
			return errors.New("duplication tool version check timed out")
		}

		return fmt.Errorf(
			"duplication tool %q version check failed: %s",
			toolLabel(config.Tool),
			commandError(err, stderr),
		)
	}

	version := semanticVersionPattern.FindString(string(stdout))
	if version == "" {
		return fmt.Errorf("duplication tool %q did not report a semantic version", toolLabel(config.Tool))
	}

	if version != config.Version {
		return fmt.Errorf(
			"duplication tool %q version %s is unsupported; pinned version is %s",
			toolLabel(config.Tool),
			version,
			config.Version,
		)
	}

	return nil
}

func runDuplicationCategory(
	parent context.Context,
	config DuplicationConfig,
	toolPath, configPath, tempRoot string,
	category report.Category,
	files []*sourceFile,
) ([]report.Clone, error) {
	categoryRoot, selected, err := snapshotDuplicationSources(tempRoot, category, files)
	if err != nil {
		return nil, err
	}

	if len(files) == 0 {
		return []report.Clone{}, nil
	}

	reportRoot := filepath.Join(tempRoot, string(category)+"-report")
	if err := os.MkdirAll(reportRoot, 0o750); err != nil {
		return nil, fmt.Errorf("create duplication report path: %w", err)
	}

	args := []string{
		"--format", "go",
		"--min-tokens", strconv.Itoa(config.MinTokens),
		"--min-lines", strconv.Itoa(config.MinLines),
		"--mode", "mild",
		"--no-gitignore",
		"--no-colors",
		"--reporters", "json",
		"--output", reportRoot,
		"--config", configPath,
		"--max-size", "16mb",
		"--workers", "1",
		"--threshold", strings.TrimSuffix(config.Threshold, "%"),
		categoryRoot,
	}

	ctx, cancel := context.WithTimeout(parent, duplicationTimeout)
	defer cancel()
	_, stderr, commandErr := runBoundedCommand(ctx, toolPath, args, tempRoot)
	jsonPath, reportErr := findCPDReport(reportRoot)
	if reportErr != nil {
		if commandErr != nil {
			return nil, fmt.Errorf("duplication command failed for %s: %s", category, commandError(commandErr, stderr))
		}

		return nil, reportErr
	}

	if commandErr != nil {
		return nil, fmt.Errorf("duplication command failed for %s: %s", category, commandError(commandErr, stderr))
	}

	data, err := readBoundedFile(jsonPath, duplicationOutputLimit)
	if err != nil {
		return nil, fmt.Errorf("read duplication report for %s: %s", category, cleanError(err))
	}

	var parsed cpdReport
	if err := json.Unmarshal(data, &parsed); err != nil {
		return nil, fmt.Errorf("decode duplication report for %s: %w", category, err)
	}

	if parsed.Duplicates == nil {
		return nil, fmt.Errorf("decode duplication report for %s: duplicates field is missing", category)
	}

	return normalizeCPDClones(*parsed.Duplicates, category, tempRoot, selected)
}

func snapshotDuplicationSources(
	tempRoot string,
	category report.Category,
	files []*sourceFile,
) (string, map[string][]byte, error) {
	categoryRoot := filepath.Join(tempRoot, string(category))
	if err := os.MkdirAll(categoryRoot, 0o750); err != nil {
		return "", nil, fmt.Errorf("create duplication source scope: %w", err)
	}

	selected := make(map[string][]byte, len(files))
	for index := range files {
		file := files[index]
		if file.bytes > duplicationFileLimit {
			return "", nil, fmt.Errorf(
				"duplication source %q exceeds the %d-byte adapter limit",
				file.path,
				duplicationFileLimit,
			)
		}

		destination := filepath.Join(categoryRoot, filepath.FromSlash(file.path))
		if err := os.MkdirAll(filepath.Dir(destination), 0o750); err != nil {
			return "", nil, fmt.Errorf("create duplication path for %q: %s", file.path, cleanError(err))
		}

		if err := os.WriteFile(destination, file.source, 0o600); err != nil {
			return "", nil, fmt.Errorf("write duplication snapshot for %q: %s", file.path, cleanError(err))
		}

		selected[file.path] = file.source
	}

	return categoryRoot, selected, nil
}

func runBoundedCommand(ctx context.Context, tool string, args []string, dir string) ([]byte, []byte, error) {
	command := exec.CommandContext(ctx, tool, args...)
	command.Dir = dir
	stdout := &boundedBuffer{limit: duplicationOutputLimit}
	stderr := &boundedBuffer{limit: duplicationOutputLimit}
	command.Stdout = stdout
	command.Stderr = stderr
	err := command.Run()
	if errors.Is(err, errDuplicationOutputLimit) {
		return stdout.Bytes(), stderr.Bytes(), errDuplicationOutputLimit
	}

	return stdout.Bytes(), stderr.Bytes(), err
}

func findCPDReport(root string) (string, error) {
	entries, err := os.ReadDir(root)
	if err != nil {
		return "", fmt.Errorf("read duplication output directory: %s", cleanError(err))
	}

	for _, entry := range entries {
		if entry.IsDir() || !strings.HasSuffix(entry.Name(), ".json") {
			continue
		}

		return filepath.Join(root, entry.Name()), nil
	}

	return "", errors.New("duplication command produced no JSON report")
}

func normalizeCPDClones(
	duplicates []cpdDuplicate,
	category report.Category,
	snapshotRoot string,
	selected map[string][]byte,
) ([]report.Clone, error) {
	scopes := buildCloneSourceScopes(selected)
	pairs := make([]normalizedClonePair, 0, len(duplicates))
	for index := range duplicates {
		duplicate := duplicates[index]
		first, err := normalizeCPDLocation(duplicate.FirstFile, category, snapshotRoot, selected)
		if err != nil {
			return nil, err
		}

		second, err := normalizeCPDLocation(duplicate.SecondFile, category, snapshotRoot, selected)
		if err != nil {
			return nil, err
		}

		firstEndpoint, err := normalizeCloneEndpoint(first, scopes[first.Path])
		if err != nil {
			return nil, err
		}
		secondEndpoint, err := normalizeCloneEndpoint(second, scopes[second.Path])
		if err != nil {
			return nil, err
		}
		if endpointLess(&secondEndpoint, &firstEndpoint) {
			firstEndpoint, secondEndpoint = secondEndpoint, firstEndpoint
		}

		pairs = append(pairs, normalizedClonePair{
			first:    firstEndpoint,
			second:   secondEndpoint,
			category: category,
			tokens:   duplicate.Tokens,
			lines:    duplicate.Lines,
		})
	}

	assignCloneEndpointOccurrences(pairs)
	assignClonePairIDs(pairs)

	clones := make([]report.Clone, 0, len(pairs))
	for index := range pairs {
		pair := &pairs[index]
		locations := []report.Location{pair.first.location, pair.second.location}
		clones = append(clones, report.Clone{
			ID:                clonePairID(pair),
			FamilyID:          pair.familyID,
			Fingerprint:       pair.fingerprint,
			Category:          category,
			Tokens:            pair.tokens,
			Lines:             pair.lines,
			Locations:         locations,
			IdentityAmbiguous: pair.ambiguous || pair.first.ambiguous || pair.second.ambiguous,
		})
	}

	sort.Slice(clones, func(i, j int) bool { return clones[i].ID < clones[j].ID })
	return clones, nil
}

func normalizeCPDLocation(
	file cpdFile,
	category report.Category,
	snapshotRoot string,
	selected map[string][]byte,
) (report.Location, error) {
	path := filepath.ToSlash(file.Name)
	prefix := filepath.ToSlash(snapshotRoot) + "/"
	path = strings.TrimPrefix(path, prefix)
	path = strings.TrimPrefix(path, string(category)+"/")
	source, ok := selected[path]
	if !ok {
		matches := make([]string, 0, 1)
		for candidate := range selected {
			if path == candidate || strings.HasSuffix(path, "/"+candidate) {
				matches = append(matches, candidate)
			}
		}

		if len(matches) == 1 {
			path = matches[0]
			source = selected[path]
		} else {
			return report.Location{}, fmt.Errorf(
				"duplication report returned unknown source path %q",
				filepath.Base(path),
			)
		}
	}

	start := report.Position{Line: file.StartLoc.Line, Column: file.StartLoc.Column + 1}
	end := cpdEndPosition(file.EndLoc, source)
	if start.Line == 0 {
		start.Line = file.Start
	}

	if start.Column == 0 {
		start.Column = 1
	}

	if end.Line == 0 {
		end.Line = file.End
	}

	if end.Column == 0 {
		end.Column = 1
	}

	return report.Location{Path: path, Start: start, End: end}, nil
}

func cpdEndPosition(point cpdPoint, source []byte) report.Position {
	if point.Line == 0 {
		return report.Position{}
	}

	if point.Column > 0 {
		return report.Position{Line: point.Line, Column: point.Column}
	}

	if point.Line == 1 {
		return report.Position{Line: 1, Column: 1}
	}

	line := point.Line - 1
	return report.Position{Line: line, Column: sourceLineLength(source, line)}
}

func sourceLineLength(source []byte, line int) int {
	if line <= 0 {
		return 0
	}

	current := 1
	start := 0
	for offset, value := range source {
		if value != '\n' {
			continue
		}

		if current == line {
			end := offset
			if end > start && source[end-1] == '\r' {
				end--
			}

			return end - start
		}

		current++
		start = offset + 1
	}

	if current == line {
		end := len(source)
		if end > start && source[end-1] == '\r' {
			end--
		}

		return end - start
	}

	return 0
}

func commandError(err error, stderr []byte) string {
	if errors.Is(err, errDuplicationOutputLimit) {
		return errDuplicationOutputLimit.Error()
	}

	if errors.Is(err, context.DeadlineExceeded) {
		return "timed out"
	}

	if exitErr, ok := errors.AsType[*exec.ExitError](err); ok {
		return fmt.Sprintf("exit status %d", exitErr.ExitCode())
	}

	if len(stderr) > 0 {
		return "command reported an error"
	}

	if execErr, ok := errors.AsType[*exec.Error](err); ok {
		return execErr.Err.Error()
	}

	return "command failed"
}

func readBoundedFile(path string, limit int64) ([]byte, error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, fmt.Errorf("open bounded duplication report: %w", err)
	}

	read := func() ([]byte, error) {
		info, statErr := file.Stat()
		if statErr != nil {
			return nil, fmt.Errorf("stat bounded duplication report: %w", statErr)
		}

		if info.Size() > limit {
			return nil, errDuplicationOutputLimit
		}

		data, readErr := io.ReadAll(io.LimitReader(file, limit+1))
		if readErr != nil {
			return nil, fmt.Errorf("read bounded duplication report: %w", readErr)
		}

		return data, nil
	}

	data, readErr := read()
	closeErr := file.Close()
	if readErr != nil {
		return nil, readErr
	}

	if closeErr != nil {
		return nil, fmt.Errorf("close bounded duplication report: %w", closeErr)
	}

	if int64(len(data)) > limit {
		return nil, errDuplicationOutputLimit
	}

	return data, nil
}

func toolLabel(tool string) string {
	if tool == "" {
		return duplicationTool
	}

	return filepath.Base(tool)
}
