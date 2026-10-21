package analysis

import (
	"context"
	"errors"
	"fmt"
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

func runDuplication(parent context.Context, state *analysisState) (*report.Duplication, error) {
	state.writeProgressf("deburr: loading duplication config\n")
	config, files, err := configureDuplication(state)
	result := &report.Duplication{Status: report.DuplicationError, Config: state.report.Config.Duplication}
	if err != nil {
		result.Error = err.Error()
		return result, err
	}
	threshold, err := strconv.ParseFloat(config.Threshold, 64)
	if err != nil {
		thresholdErr := fmt.Errorf("parse configured duplication threshold: %w", err)
		result.Error = thresholdErr.Error()
		return result, thresholdErr
	}

	toolPath, err := resolveDuplicationTool(state.cfg.Duplication.Tool)
	if err != nil {
		result.Error = err.Error()
		return result, err
	}

	state.writeProgressf("deburr: checking duplication detector\n")
	version, err := verifyDuplicationTool(parent, state.cfg.Duplication, toolPath)
	result.Config.Version = version
	result.Config.Tool = toolLabel(toolPath)
	state.report.Config.Duplication = result.Config
	if err != nil {
		result.Error = err.Error()
		return result, err
	}

	percentages, clones, err := measureDuplicationCategories(parent, state, config, toolPath, version, files)
	if err != nil {
		result.Error = err.Error()
		return result, err
	}

	result.Percentages = percentages
	result.Status = report.DuplicationMeasured
	result.Clones = clones
	for _, measured := range result.Percentages {
		if measured.Percentage > threshold {
			result.ThresholdExceeded = true
		}
	}

	if config.EnforceThreshold && result.ThresholdExceeded {
		return result, fmt.Errorf("duplication exceeds configured threshold %s%%", config.Threshold)
	}

	return result, nil
}

func measureDuplicationCategories(
	parent context.Context,
	state *analysisState,
	config DuplicationConfig,
	toolPath, version string,
	files []*sourceFile,
) ([]report.DuplicationPercentage, []report.Clone, error) {
	state.writeProgressf(
		"deburr: using %s %s; %d files selected for duplication\n",
		toolLabel(toolPath),
		version,
		len(files),
	)
	tempRoot, err := os.MkdirTemp("", duplicationSnapshotPrefix)
	if err != nil {
		return nil, nil, fmt.Errorf("create duplication snapshot: %w", err)
	}

	configPath := filepath.Join(tempRoot, "cpd-config.json")
	if writeErr := os.WriteFile(configPath, config.DetectorSettings, 0o600); writeErr != nil {
		err = fmt.Errorf("write duplication config: %s", cleanError(writeErr))
	}

	var percentages []report.DuplicationPercentage
	var clones []report.Clone
	if err == nil {
		percentages, clones, err = collectDuplicationCategories(
			parent,
			state,
			config,
			toolPath,
			tempRoot,
			configPath,
			files,
		)
	}

	cleanupErr := os.RemoveAll(tempRoot)
	if cleanupErr != nil {
		cleanupFailure := fmt.Errorf("remove duplication snapshot: %s", cleanError(cleanupErr))
		if err == nil {
			err = cleanupFailure
		} else {
			err = fmt.Errorf("%w; %w", err, cleanupFailure)
		}
	}

	if err != nil {
		return nil, nil, err
	}

	return percentages, clones, nil
}

func collectDuplicationCategories(
	parent context.Context,
	state *analysisState,
	config DuplicationConfig,
	toolPath, tempRoot, configPath string,
	files []*sourceFile,
) ([]report.DuplicationPercentage, []report.Clone, error) {
	groups := duplicationGroups(files)
	categories := []report.Category{report.CategoryProduction, report.CategoryTest}
	percentages := make([]report.DuplicationPercentage, 0, len(categories))
	clones := make([]report.Clone, 0)
	for _, category := range categories {
		categoryFiles := groups[category]
		sort.Slice(categoryFiles, func(i, j int) bool { return categoryFiles[i].path < categoryFiles[j].path })
		measurement, err := runDuplicationCategory(
			parent,
			config,
			toolPath,
			configPath,
			tempRoot,
			category,
			categoryFiles,
			state,
		)
		if err != nil {
			return nil, nil, err
		}

		state.writeProgressf(
			"deburr: %s duplication finished: %.2f%%; %d of %d files analyzed\n",
			category,
			measurement.percentage,
			measurement.files,
			len(categoryFiles),
		)
		clones = append(clones, measurement.clones...)
		percentages = append(percentages, report.DuplicationPercentage{
			Category:      category,
			Percentage:    measurement.percentage,
			Files:         measurement.files,
			SelectedFiles: len(categoryFiles),
		})
	}

	sort.Slice(clones, func(i, j int) bool { return clones[i].ID < clones[j].ID })
	return percentages, clones, nil
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
	automatic := tool == ""
	if automatic {
		tool = duplicationTool
	}

	path, err := exec.LookPath(tool)
	if err != nil && automatic {
		path, err = exec.LookPath("jscpd")
	}

	if err != nil {
		return "", fmt.Errorf("duplication tool %q is unavailable", toolLabel(tool))
	}

	path, err = filepath.Abs(path)
	if err != nil {
		return "", fmt.Errorf("resolve duplication tool %q: %s", toolLabel(tool), cleanError(err))
	}

	return path, nil
}

func verifyDuplicationTool(parent context.Context, config DuplicationConfig, toolPath string) (string, error) {
	ctx, cancel := context.WithTimeout(parent, duplicationTimeout)
	defer cancel()
	stdout, stderr, err := runBoundedCommand(ctx, toolPath, []string{"--version"}, "")
	if err != nil {
		if errors.Is(err, context.DeadlineExceeded) || errors.Is(ctx.Err(), context.DeadlineExceeded) {
			return "", errors.New("duplication tool version check timed out")
		}

		return "", fmt.Errorf(
			"duplication tool %q version check failed: %s",
			toolLabel(config.Tool),
			commandError(err, stderr),
		)
	}

	version := semanticVersionPattern.FindString(string(stdout))
	if version == "" {
		return "", fmt.Errorf("duplication tool %q did not report a semantic version", toolLabel(config.Tool))
	}

	if strings.Split(version, ".")[0] != duplicationToolMajor {
		return version, fmt.Errorf(
			"duplication tool %q version %s is unsupported; supported major is %s",
			toolLabel(config.Tool),
			version,
			duplicationToolMajor,
		)
	}

	return version, nil
}

func toolLabel(tool string) string {
	if tool == "" {
		return duplicationTool
	}

	return filepath.Base(tool)
}
