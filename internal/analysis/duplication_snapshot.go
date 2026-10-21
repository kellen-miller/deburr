package analysis

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"

	"github.com/kellen-miller/deburr/internal/report"
)

type cpdTotalStatistics struct {
	Percentage *float64 `json:"percentage"`
	Sources    *int     `json:"sources"`
}

type cpdStatistics struct {
	Total cpdTotalStatistics `json:"total"`
}

type cpdReport struct {
	Statistics cpdStatistics   `json:"statistics"`
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

type duplicationMeasurement struct {
	clones     []report.Clone
	percentage float64
	files      int
}

func runDuplicationCategory(
	parent context.Context,
	config DuplicationConfig,
	toolPath, configPath, tempRoot string,
	category report.Category,
	files []*sourceFile,
	state *analysisState,
) (duplicationMeasurement, error) {
	state.writeProgressf("deburr: snapshotting %d %s files\n", len(files), category)
	categoryRoot, selected, err := snapshotDuplicationSources(tempRoot, category, files)
	if err != nil {
		return duplicationMeasurement{}, err
	}

	if len(files) == 0 {
		return duplicationMeasurement{clones: []report.Clone{}}, nil
	}

	reportRoot := filepath.Join(tempRoot, string(category)+"-report")
	if err := os.MkdirAll(reportRoot, 0o750); err != nil {
		return duplicationMeasurement{}, fmt.Errorf("create duplication report path: %w", err)
	}

	args := []string{
		"--format", config.Formats,
		"--min-tokens", strconv.Itoa(config.MinTokens),
		"--min-lines", strconv.Itoa(config.MinLines),
		"--mode", config.Mode,
		"--no-gitignore",
		"--no-colors",
		"--reporters", "json",
		"--output", reportRoot,
		"--config", configPath,
		"--max-size", "16mb",
		"--workers", "1",
		"--threshold", "100",
		categoryRoot,
	}

	state.writeProgressf("deburr: running %s for %s duplication\n", toolLabel(toolPath), category)
	ctx, cancel := context.WithTimeout(parent, duplicationTimeout)
	defer cancel()
	_, stderr, commandErr := runBoundedCommand(ctx, toolPath, args, tempRoot)
	jsonPath, reportErr := findCPDReport(reportRoot)
	if reportErr != nil {
		if commandErr != nil {
			return duplicationMeasurement{}, fmt.Errorf(
				"duplication command failed for %s: %s",
				category,
				commandError(commandErr, stderr),
			)
		}

		return duplicationMeasurement{}, reportErr
	}

	if commandErr != nil {
		return duplicationMeasurement{}, fmt.Errorf(
			"duplication command failed for %s: %s",
			category,
			commandError(commandErr, stderr),
		)
	}

	measurement, err := readDuplicationMeasurement(jsonPath, category, tempRoot, selected, files)
	if err != nil {
		return duplicationMeasurement{}, err
	}

	return measurement, nil
}

func readDuplicationMeasurement(
	jsonPath string,
	category report.Category,
	tempRoot string,
	selected map[string][]byte,
	files []*sourceFile,
) (duplicationMeasurement, error) {
	data, err := readBoundedFile(jsonPath, duplicationOutputLimit)
	if err != nil {
		return duplicationMeasurement{}, fmt.Errorf("read duplication report for %s: %s", category, cleanError(err))
	}

	var parsed cpdReport
	if err := json.Unmarshal(data, &parsed); err != nil {
		return duplicationMeasurement{}, fmt.Errorf("decode duplication report for %s: %w", category, err)
	}

	if parsed.Duplicates == nil {
		return duplicationMeasurement{}, fmt.Errorf(
			"decode duplication report for %s: duplicates field is missing",
			category,
		)
	}

	if parsed.Statistics.Total.Percentage == nil {
		return duplicationMeasurement{}, fmt.Errorf("duplication report for %s omitted measured percentage", category)
	}
	percentage := *parsed.Statistics.Total.Percentage

	if math.IsNaN(percentage) || math.IsInf(percentage, 0) || percentage < 0 || percentage > 100 {
		return duplicationMeasurement{}, fmt.Errorf(
			"duplication report for %s has an invalid measured percentage",
			category,
		)
	}

	if parsed.Statistics.Total.Sources == nil || *parsed.Statistics.Total.Sources <= 0 ||
		*parsed.Statistics.Total.Sources > len(files) {
		return duplicationMeasurement{}, fmt.Errorf(
			"duplication report for %s has no valid analyzed source count; check detector min/max lines and other filters",
			category,
		)
	}

	clones, err := normalizeCPDClones(*parsed.Duplicates, category, tempRoot, selected, buildCloneSourceScopes(files))
	if err != nil {
		return duplicationMeasurement{}, fmt.Errorf("normalize duplication report for %s: %w", category, err)
	}

	return duplicationMeasurement{clones: clones, percentage: percentage, files: *parsed.Statistics.Total.Sources}, nil
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
