package analysis

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

type duplicationOptions struct {
	settings         map[string]json.RawMessage
	pattern          string
	mode             string
	paths            []string
	ignores          []string
	formats          []string
	config           DuplicationConfig
	maxBytes         int64
	respectGitignore bool
}

// Detector configuration narrows duplication scope only; native coverage stays visible.
func configureDuplication(state *analysisState) (DuplicationConfig, []*sourceFile, error) {
	root, configPath, err := resolveDuplicationConfig(state.base, state.cfg.Duplication.ConfigPath)
	if err != nil {
		return state.cfg.Duplication, nil, err
	}

	options := defaultDuplicationOptions(state.cfg.Duplication)
	if configPath != "" {
		if err := readDuplicationOptions(configPath, &options); err != nil {
			return options.config, nil, err
		}

		root = filepath.Dir(configPath)
	}

	if err := normalizeDuplicationOptions(&options); err != nil {
		return options.config, nil, err
	}

	ignoreRoot, err := duplicationIgnoreRoot(root, state.base)
	if err != nil {
		return options.config, nil, err
	}

	selected, ignoreFiles, err := selectDuplicationSources(state, root, ignoreRoot, &options)
	if err != nil {
		return options.config, nil, err
	}

	if len(selected) == 0 {
		return options.config, nil, errors.New(
			"duplication config selects no eligible sources; check paths, ignores, formats, and size limits",
		)
	}

	recordDuplicationSummary(state, root, ignoreRoot, ignoreFiles, &options)
	return options.config, selected, nil
}

func resolveDuplicationConfig(base, configPath string) (string, string, error) {
	root := base
	if configPath != "" {
		absolute, err := filepath.Abs(configPath)
		if err != nil {
			return root, "", fmt.Errorf("resolve duplication config: %w", err)
		}

		return root, absolute, nil
	}

	for directory := root; ; directory = filepath.Dir(directory) {
		candidate := filepath.Join(directory, ".jscpd.json")
		if _, err := os.Stat(candidate); err == nil {
			return root, candidate, nil
		} else if !errors.Is(err, os.ErrNotExist) {
			return root, "", fmt.Errorf("discover duplication config: %s", cleanError(err))
		}

		if _, err := os.Stat(filepath.Join(directory, ".git")); err == nil {
			return directory, "", nil
		}

		if directory == filepath.Dir(directory) {
			return root, "", nil
		}
	}
}

func recordDuplicationSummary(
	state *analysisState,
	root, ignoreRoot string,
	ignoreFiles map[string][]byte,
	options *duplicationOptions,
) {
	ignoreNames := make([]string, 0, len(ignoreFiles))
	for path := range ignoreFiles {
		if ignoreFiles[path] != nil {
			ignoreNames = append(ignoreNames, path)
		}
	}

	sort.Strings(ignoreNames)
	var ignoreIdentity strings.Builder
	if relativeRoot, err := filepath.Rel(state.base, ignoreRoot); err == nil {
		ignoreIdentity.WriteString(filepath.ToSlash(relativeRoot) + "\x00")
	}

	for _, path := range ignoreNames {
		ignoreIdentity.WriteString(path + "\x00" + string(ignoreFiles[path]) + "\x00")
	}

	sort.Strings(options.paths)
	sort.Strings(options.ignores)
	summary := &state.report.Config.Duplication
	if relativeRoot, err := filepath.Rel(state.base, root); err == nil {
		summary.ScopeRoot = filepath.ToSlash(relativeRoot)
	}

	summary.Formats, summary.MaxFileBytes = options.formats, options.maxBytes
	summary.MinTokens, summary.MinLines, summary.Threshold = options.config.MinTokens, options.config.MinLines, options.config.Threshold
	summary.Mode, summary.Paths, summary.Ignore, summary.Pattern = options.mode, options.paths, options.ignores, options.pattern
	summary.Gitignore, summary.EnforceThreshold = options.respectGitignore, options.config.EnforceThreshold
	summary.DetectorSettings = options.config.DetectorSettings
	if options.respectGitignore {
		summary.GitignoreDigest = "sha256:" + digestText(ignoreIdentity.String())
	}
}
