package cli

import (
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"slices"
	"strings"

	"github.com/kellen-miller/deburr/internal/render"
	"github.com/kellen-miller/deburr/internal/report"
)

func writeAuditReport(value *report.Report, target string, options *commandOptions, stdout io.Writer) error {
	if options.output != "" {
		return writeOutputFile(options.output, func(w io.Writer) error {
			if options.format == render.FormatGitHub {
				value = withGitHubPaths(value, target)
			}

			if err := render.Write(w, value, options.format); err != nil {
				return fmt.Errorf("write report: %w", err)
			}

			return nil
		})
	}

	if options.format == render.FormatGitHub {
		value = withGitHubPaths(value, target)
	}

	if err := render.Write(stdout, value, options.format); err != nil {
		return fmt.Errorf("write report: %w", err)
	}

	return nil
}

func writeOutputFile(path string, renderReport func(io.Writer) error) error {
	if path == "-" {
		return errors.New("--output - is not supported; omit --output for stdout")
	}

	absolute, err := filepath.Abs(filepath.Clean(path))
	if err != nil {
		return fmt.Errorf("resolve output %q: %w", path, err)
	}

	parent := filepath.Dir(absolute)
	file, err := os.CreateTemp(parent, ".deburr-report-*")
	if err != nil {
		return fmt.Errorf("open output %q: %w", path, err)
	}

	temporary := file.Name()
	if err := renderReport(file); err != nil {
		closeErr := file.Close()
		return cleanupOutput(temporary, errors.Join(err, closeErr))
	}

	if err := file.Close(); err != nil {
		return cleanupOutput(temporary, fmt.Errorf("close output %q: %w", path, err))
	}

	if err := os.Rename(temporary, absolute); err != nil {
		return cleanupOutput(temporary, fmt.Errorf("replace output %q: %w", path, err))
	}

	return nil
}

func cleanupOutput(path string, cause error) error {
	if err := os.Remove(path); err != nil && !errors.Is(err, os.ErrNotExist) {
		return errors.Join(cause, fmt.Errorf("remove temporary output %q: %w", path, err))
	}

	return cause
}

func validateOutputPath(target, output string) error {
	if output == "-" {
		return errors.New("--output - is not supported; omit --output for stdout")
	}

	outputAbs, err := filepath.Abs(filepath.Clean(output))
	if err != nil {
		return fmt.Errorf("resolve output path: %w", err)
	}

	targetAbs, err := filepath.Abs(filepath.Clean(target))
	if err != nil {
		return fmt.Errorf("resolve target path: %w", err)
	}

	info, err := os.Stat(targetAbs)
	if err != nil {
		return fmt.Errorf("stat target %q: %w", target, err)
	}

	targetResolved, err := filepath.EvalSymlinks(targetAbs)
	if err != nil {
		return fmt.Errorf("resolve target path: %w", err)
	}

	outputResolved, err := resolveOutputPath(outputAbs)
	if err != nil {
		return fmt.Errorf("resolve output path: %w", err)
	}

	if info.IsDir() {
		if insidePath(targetResolved, outputResolved) {
			return fmt.Errorf("refusing --output %q: output must be outside the audit target", output)
		}
	} else if filepath.Clean(targetResolved) == filepath.Clean(outputResolved) || samePath(targetAbs, outputAbs) {
		return fmt.Errorf("refusing --output %q: it is the audit input", output)
	}

	if outputLink, linkErr := os.Lstat(outputAbs); linkErr == nil && outputLink.Mode()&os.ModeSymlink != 0 {
		return fmt.Errorf("refusing --output %q: final output path is a symlink", output)
	}

	if outputInfo, statErr := os.Stat(outputAbs); statErr == nil && outputInfo.IsDir() {
		return fmt.Errorf("output %q is a directory", output)
	}

	return nil
}

func resolveOutputPath(path string) (string, error) {
	current := path
	missing := make([]string, 0)
	for {
		if _, err := os.Lstat(current); err == nil {
			resolved, resolveErr := filepath.EvalSymlinks(current)
			if resolveErr != nil {
				return "", fmt.Errorf("resolve %q: %w", current, resolveErr)
			}

			for index := range slices.Backward(missing) {
				resolved = filepath.Join(resolved, missing[index])
			}

			return filepath.Clean(resolved), nil
		} else if !os.IsNotExist(err) {
			return "", fmt.Errorf("stat %q: %w", current, err)
		}

		parent := filepath.Dir(current)
		if parent == current {
			return "", errors.New("output has no existing parent")
		}

		missing = append(missing, filepath.Base(current))
		current = parent
	}
}

func validateCompareOutputPath(output string, inputs ...string) error {
	if output == "-" {
		return nil
	}

	outputAbs, err := filepath.Abs(filepath.Clean(output))
	if err != nil {
		return fmt.Errorf("resolve output path: %w", err)
	}

	for _, input := range inputs {
		inputAbs, inputErr := filepath.Abs(filepath.Clean(input))
		if inputErr != nil {
			return fmt.Errorf("resolve report path %q: %w", input, inputErr)
		}

		if samePath(inputAbs, outputAbs) {
			return fmt.Errorf("refusing --output %q: it would overwrite input report %q", output, input)
		}

		inputInfo, inputStatErr := os.Stat(inputAbs)
		outputInfo, outputStatErr := os.Stat(outputAbs)
		if inputStatErr == nil && outputStatErr == nil && os.SameFile(inputInfo, outputInfo) {
			return fmt.Errorf("refusing --output %q: it aliases input report %q", output, input)
		}
	}

	if outputInfo, statErr := os.Stat(outputAbs); statErr == nil && outputInfo.IsDir() {
		return fmt.Errorf("output %q is a directory", output)
	}

	return nil
}

func insidePath(root, candidate string) bool {
	rel, err := filepath.Rel(root, candidate)
	return err == nil && rel != ".." && !strings.HasPrefix(rel, ".."+string(filepath.Separator))
}

func samePath(left, right string) bool {
	leftResolved, leftErr := filepath.EvalSymlinks(left)
	rightResolved, rightErr := filepath.EvalSymlinks(right)
	if leftErr == nil && rightErr == nil {
		return filepath.Clean(leftResolved) == filepath.Clean(rightResolved)
	}

	return filepath.Clean(left) == filepath.Clean(right)
}

func withGitHubPaths(value *report.Report, target string) *report.Report {
	prefix := githubPathPrefix(target)
	if value == nil || prefix == "" {
		return value
	}

	copyValue := *value
	copyValue.Findings = append([]report.Finding(nil), value.Findings...)
	for index := range copyValue.Findings {
		copyValue.Findings[index].Path = filepath.ToSlash(
			filepath.Join(prefix, filepath.FromSlash(copyValue.Findings[index].Path)),
		)
	}

	copyValue.Files = append([]report.File(nil), value.Files...)
	for index := range copyValue.Files {
		copyValue.Files[index].Path = filepath.ToSlash(
			filepath.Join(prefix, filepath.FromSlash(copyValue.Files[index].Path)),
		)
	}

	if value.Duplication != nil {
		duplication := *value.Duplication
		duplication.Clones = append([]report.Clone(nil), value.Duplication.Clones...)
		for cloneIndex := range duplication.Clones {
			duplication.Clones[cloneIndex].Locations = append(
				[]report.Location(nil),
				value.Duplication.Clones[cloneIndex].Locations...)
			for locationIndex := range duplication.Clones[cloneIndex].Locations {
				location := &duplication.Clones[cloneIndex].Locations[locationIndex]
				location.Path = filepath.ToSlash(filepath.Join(prefix, filepath.FromSlash(location.Path)))
			}
		}

		copyValue.Duplication = &duplication
	}

	return &copyValue
}

func githubPathPrefix(target string) string {
	workspace := os.Getenv("GITHUB_WORKSPACE")
	if workspace == "" {
		var err error
		workspace, err = os.Getwd()
		if err != nil {
			return ""
		}
	}

	workspace, workspaceErr := filepath.Abs(filepath.Clean(workspace))
	target, targetErr := filepath.Abs(filepath.Clean(target))
	if workspaceErr != nil || targetErr != nil {
		return ""
	}

	if info, err := os.Stat(target); err == nil && !info.IsDir() {
		target = filepath.Dir(target)
	}

	rel, err := filepath.Rel(workspace, target)
	if err != nil || rel == "." || rel == ".." || strings.HasPrefix(rel, ".."+string(filepath.Separator)) {
		return ""
	}

	return filepath.ToSlash(rel)
}

func exitWithError(w io.Writer, code int, err error) int {
	if writeErr := writeError(w, err); writeErr != nil {
		return exitFailure
	}

	return code
}

func writeError(w io.Writer, err error) error {
	if _, writeErr := fmt.Fprintf(w, "deburr: %s\n", err); writeErr != nil {
		return fmt.Errorf("write diagnostic: %w", writeErr)
	}

	return nil
}
