package analysis

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"

	"github.com/bmatcuk/doublestar/v4"
	"github.com/go-git/go-git/v5/plumbing/format/gitignore"
	"github.com/kellen-miller/deburr/internal/report"
)

func duplicationIgnoreRoot(root, base string) (string, error) {
	ignoreRoot := base
	relative, err := filepath.Rel(root, base)
	if err != nil {
		return "", fmt.Errorf("resolve duplication ignore root: %w", err)
	}
	if relative != ".." && !strings.HasPrefix(relative, ".."+string(filepath.Separator)) {
		ignoreRoot = root
	}

	for directory := base; ; directory = filepath.Dir(directory) {
		if _, err := os.Stat(filepath.Join(directory, ".git")); err == nil {
			ignoreRoot = directory
			break
		}
		if directory == filepath.Dir(directory) {
			break
		}
	}

	return ignoreRoot, nil
}

func selectDuplicationSources(
	state *analysisState,
	root, ignoreRoot string,
	options *duplicationOptions,
) ([]*sourceFile, map[string][]byte, error) {
	selected := make([]*sourceFile, 0, len(state.files))
	ignoreFiles := make(map[string][]byte)
	for _, file := range state.files {
		include, err := selectDuplicationSource(file, state.base, root, ignoreRoot, options, ignoreFiles)
		if err != nil {
			return nil, nil, err
		}
		if include {
			selected = append(selected, file)
		}
	}

	return selected, ignoreFiles, nil
}

func selectDuplicationSource(
	file *sourceFile,
	base, root, ignoreRoot string,
	options *duplicationOptions,
	ignoreFiles map[string][]byte,
) (bool, error) {
	if file.status != report.FileAnalyzed ||
		(file.category != report.CategoryProduction && file.category != report.CategoryTest) {
		return false, nil
	}

	format := duplicationFormat(file.path)
	if !slices.Contains(options.formats, format) || file.bytes > options.maxBytes {
		return false, nil
	}

	original := filepath.Join(base, filepath.FromSlash(file.path))
	relative, err := filepath.Rel(root, original)
	if err != nil {
		return false, fmt.Errorf("resolve duplication source %q relative to config: %w", file.path, err)
	}
	relative = filepath.ToSlash(relative)

	return matchesDuplicationScope(file.path, relative, original, ignoreRoot, options, ignoreFiles)
}

func duplicationFormat(path string) string {
	switch filepath.Ext(path) {
	case ".go":
		return duplicationFormatGo
	case typeScriptReactExtension:
		return duplicationFormatTSX
	default:
		return duplicationFormatTypeScript
	}
}

func matchesDuplicationScope(
	path, relative, original, ignoreRoot string,
	options *duplicationOptions,
	ignoreFiles map[string][]byte,
) (bool, error) {
	if len(options.paths) > 0 {
		matched, err := matchesDuplicationPath(relative, options.paths)
		if err != nil {
			return false, fmt.Errorf("match duplication path %q: %w", path, err)
		}
		if !matched {
			return false, nil
		}
	}

	ignored, err := matchesDuplicationPath(relative, options.ignores)
	if err != nil {
		return false, fmt.Errorf("match duplication ignore path %q: %w", path, err)
	}
	if ignored {
		return false, nil
	}

	if options.pattern != "" {
		matched, err := doublestar.Match(options.pattern, relative)
		if err != nil {
			return false, fmt.Errorf("match duplication pattern %q: %w", options.pattern, err)
		}
		if !matched {
			return false, nil
		}
	}

	if options.respectGitignore {
		ignoreRelative, err := filepath.Rel(ignoreRoot, original)
		if err != nil {
			return false, fmt.Errorf("resolve duplication gitignore source %q: %w", path, err)
		}
		gitignored, err := duplicationGitignored(ignoreRoot, filepath.ToSlash(ignoreRelative), ignoreFiles)
		if err != nil {
			return false, fmt.Errorf("check duplication gitignore for %q: %w", path, err)
		}
		if gitignored {
			return false, nil
		}
	}

	return true, nil
}

func matchesDuplicationPath(path string, patterns []string) (bool, error) {
	for _, pattern := range patterns {
		pattern = strings.TrimSuffix(strings.TrimPrefix(filepath.ToSlash(pattern), "./"), "/")
		if pattern == "." || path == pattern || strings.HasPrefix(path, pattern+"/") {
			return true, nil
		}

		matched, err := doublestar.Match(pattern, path)
		if err != nil {
			return false, fmt.Errorf("match path pattern %q: %w", pattern, err)
		}
		if matched {
			return true, nil
		}
	}

	return false, nil
}

func duplicationGitignored(root, path string, cache map[string][]byte) (bool, error) {
	parts := strings.Split(path, "/")
	patterns := make([]gitignore.Pattern, 0)
	for depth := range parts {
		domain := parts[:depth]
		if depth > 0 && gitignore.NewMatcher(patterns).Match(domain, true) {
			return true, nil
		}

		ignorePath := filepath.ToSlash(filepath.Join(strings.Join(domain, "/"), ".gitignore"))
		data, exists := cache[ignorePath]
		if !exists {
			var err error
			data, err = readBoundedFile(filepath.Join(root, filepath.FromSlash(ignorePath)), duplicationConfigMaxBytes)
			if err != nil && !errors.Is(err, os.ErrNotExist) {
				return false, fmt.Errorf("read duplication gitignore %q: %s", ignorePath, cleanError(err))
			}

			cache[ignorePath] = data
		}

		for line := range strings.SplitSeq(string(data), "\n") {
			line = strings.TrimRight(line, "\r")
			if strings.TrimSpace(line) == "" || strings.HasPrefix(line, "#") {
				continue
			}

			patterns = append(patterns, gitignore.ParsePattern(line, domain))
		}
	}

	return gitignore.NewMatcher(patterns).Match(parts, false), nil
}
