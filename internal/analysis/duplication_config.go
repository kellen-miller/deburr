package analysis

import (
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"

	"github.com/bmatcuk/doublestar/v4"
	"github.com/go-git/go-git/v5/plumbing/format/gitignore"
	"github.com/kellen-miller/deburr/internal/report"
)

// Detector configuration narrows duplication scope only; native coverage stays visible.
func configureDuplication(state *analysisState) (DuplicationConfig, []*sourceFile, error) {
	config := state.cfg.Duplication
	root := state.base
	configPath := config.ConfigPath
	if configPath != "" {
		absolute, err := filepath.Abs(configPath)
		if err != nil {
			return config, nil, fmt.Errorf("resolve duplication config: %w", err)
		}

		configPath = absolute
	} else {
		for directory := root; ; directory = filepath.Dir(directory) {
			candidate := filepath.Join(directory, ".jscpd.json")
			if _, err := os.Stat(candidate); err == nil {
				configPath = candidate
				break
			} else if !errors.Is(err, os.ErrNotExist) {
				return config, nil, fmt.Errorf("discover duplication config: %s", cleanError(err))
			}

			if _, err := os.Stat(filepath.Join(directory, ".git")); err == nil {
				root = directory
				break
			}

			if directory == filepath.Dir(directory) {
				break
			}
		}
	}

	settings := make(map[string]json.RawMessage)
	paths, ignores := []string{}, []string{}
	formats := []string{"go", "typescript", "tsx"}
	maxBytes := int64(duplicationFileLimit)
	pattern, mode := "", "mild"
	respectGitignore := true
	if configPath != "" {
		root = filepath.Dir(configPath)
		data, err := readBoundedFile(configPath, 1<<20)
		if err != nil {
			return config, nil, fmt.Errorf("read duplication config: %s", cleanError(err))
		}

		if err := json.Unmarshal(data, &settings); err != nil || settings == nil {
			return config, nil, errors.New("duplication config must be a JSON object")
		}

		// Scope is applied to original paths. Output, exit policy, and execution
		// settings belong to Deburr and cannot redirect the snapshot command.
		keys := make([]string, 0, len(settings))
		for key := range settings {
			keys = append(keys, key)
		}

		sort.Strings(keys)
		for _, key := range keys {
			value := settings[key]
			var err error
			switch key {
			case "path":
				err = json.Unmarshal(value, &paths)
			case "format":
				err = json.Unmarshal(value, &formats)
				if err != nil {
					var format string
					err = json.Unmarshal(value, &format)
					formats = strings.Split(format, ",")
				}

			case "maxSize":
				err = json.Unmarshal(value, &maxBytes)
				if err != nil {
					var size string
					err = json.Unmarshal(value, &size)
					if err == nil {
						size = strings.ToLower(strings.TrimSpace(size))
						multiplier := int64(1)
						for suffix, bytes := range map[string]int64{"kb": 1024, "mb": 1024 * 1024, "gb": 1024 * 1024 * 1024} {
							if strings.HasSuffix(size, suffix) {
								multiplier = bytes
								size = strings.TrimSuffix(size, suffix)
								break
							}
						}

						var number int64
						number, err = strconv.ParseInt(strings.TrimSuffix(size, "b"), 10, 64)
						if err == nil && (number <= 0 || number > math.MaxInt64/multiplier) {
							err = errors.New("size must be positive and fit within int64")
						}

						maxBytes = number * multiplier
					}
				}

				if err == nil && maxBytes <= 0 {
					err = errors.New("must be positive")
				}

			case "ignore":
				err = json.Unmarshal(value, &ignores)
			case "gitignore":
				err = json.Unmarshal(value, &respectGitignore)
			case "pattern":
				err = json.Unmarshal(value, &pattern)
			case "minTokens":
				err = json.Unmarshal(value, &config.MinTokens)
			case "minLines":
				err = json.Unmarshal(value, &config.MinLines)
			case "threshold":
				var threshold float64
				err = json.Unmarshal(value, &threshold)
				if threshold < 0 || threshold > 100 {
					err = errors.New("must be between 0 and 100")
				}

				config.Threshold = strconv.FormatFloat(threshold, 'f', -1, 64)
			case "mode":
				err = json.Unmarshal(value, &mode)
			case "reporters", "output", "silent", "noColors", "noTips", "absolute", "workers", "exitCode":
				// Deburr owns reporting, bounded execution, and failure policy.
			case "maxLines":
				var maximum int
				err = json.Unmarshal(value, &maximum)
				if err == nil && maximum <= 0 {
					err = errors.New("must be positive")
				}

				if err == nil {
					continue
				}

			case "skipLocal":
				var enabled bool
				err = json.Unmarshal(value, &enabled)
				if err == nil && enabled {
					err = errors.New("true is unsupported: isolated category snapshots cannot preserve detector path groups")
				}

			case "ignoreCase", "ignoreIdentifiers", "ignoreLiterals", "ignoreAnnotations":
				var enabled bool
				err = json.Unmarshal(value, &enabled)
				if err == nil {
					continue
				}

			case "ignorePattern":
				var patterns []string
				err = json.Unmarshal(value, &patterns)
				if err == nil {
					continue
				}
			default:
				err = errors.New("unsupported setting")
			}

			if err != nil {
				return config, nil, fmt.Errorf("duplication config %q: %w", key, err)
			}

			delete(settings, key)
		}
	}

	if config.MinTokens <= 0 || config.MinLines <= 0 {
		return config, nil, errors.New("duplication minimum tokens and lines must be positive")
	}

	if mode != "mild" && mode != "weak" && mode != "strict" {
		return config, nil, errors.New("duplication mode must be mild, weak, or strict")
	}

	threshold, err := strconv.ParseFloat(strings.TrimSuffix(config.Threshold, "%"), 64)
	if err != nil || math.IsNaN(threshold) || math.IsInf(threshold, 0) || threshold < 0 || threshold > 100 {
		return config, nil, errors.New("duplication threshold must be between 0 and 100")
	}

	config.Threshold = strconv.FormatFloat(threshold, 'f', -1, 64)
	globs := append(append([]string{}, paths...), ignores...)
	if pattern != "" {
		globs = append(globs, pattern)
	}

	for _, glob := range globs {
		if glob == "" || filepath.IsAbs(glob) {
			return config, nil, errors.New("duplication config paths must be non-empty and relative")
		}

		if !doublestar.ValidatePattern(glob) {
			return config, nil, fmt.Errorf("duplication config has invalid path pattern %q", glob)
		}
	}

	if len(formats) == 0 {
		formats = []string{"go", "typescript", "tsx"}
	}

	formatSet := make(map[string]bool)
	for _, format := range formats {
		formatSet[strings.TrimSpace(format)] = true
	}

	formats = make([]string, 0, 3)
	for _, format := range []string{"go", "typescript", "tsx"} {
		if formatSet[format] {
			formats = append(formats, format)
		}
	}

	if len(formats) == 0 {
		return config, nil, errors.New("duplication config selects no supported formats")
	}

	config.Formats = strings.Join(formats, ",")
	config.Mode = mode
	config.DetectorSettings, err = json.Marshal(settings)
	if err != nil {
		return config, nil, fmt.Errorf("encode effective duplication settings: %w", err)
	}

	// Git ignore inheritance starts at the repository, independently of where
	// the detector config lives. Without a repository, include a containing
	// config directory when auditing one of its subdirectories.
	ignoreRoot := state.base
	if relative, err := filepath.Rel(root, state.base); err == nil && relative != ".." && !strings.HasPrefix(relative, ".."+string(filepath.Separator)) {
		ignoreRoot = root
	}

	for directory := state.base; ; directory = filepath.Dir(directory) {
		if _, err := os.Stat(filepath.Join(directory, ".git")); err == nil {
			ignoreRoot = directory
			break
		}

		if directory == filepath.Dir(directory) {
			break
		}
	}

	selected := make([]*sourceFile, 0)
	ignoreFiles := make(map[string][]byte)
	for _, file := range state.files {
		if file.status != report.FileAnalyzed || (file.category != report.CategoryProduction && file.category != report.CategoryTest) {
			continue
		}

		format := "typescript"
		switch filepath.Ext(file.path) {
		case ".go":
			format = "go"
		case ".tsx":
			format = "tsx"
		}

		if !formatSet[format] || file.bytes > maxBytes {
			continue
		}

		original := filepath.Join(state.base, filepath.FromSlash(file.path))
		relative, err := filepath.Rel(root, original)
		if err != nil {
			return config, nil, fmt.Errorf("resolve duplication source %q relative to config: %w", file.path, err)
		}

		relative = filepath.ToSlash(relative)
		if len(paths) > 0 && !matchesDuplicationPath(relative, paths) {
			continue
		}

		if matchesDuplicationPath(relative, ignores) {
			continue
		}

		if pattern != "" {
			matched, _ := doublestar.Match(pattern, relative)
			if !matched {
				continue
			}
		}

		if respectGitignore {
			ignoreRelative, err := filepath.Rel(ignoreRoot, original)
			if err != nil {
				return config, nil, fmt.Errorf("resolve duplication gitignore source %q: %w", file.path, err)
			}

			ignored, err := duplicationGitignored(ignoreRoot, filepath.ToSlash(ignoreRelative), ignoreFiles)
			if err != nil {
				return config, nil, err
			}

			if ignored {
				continue
			}
		}

		selected = append(selected, file)
	}

	if len(selected) == 0 {
		return config, nil, errors.New("duplication config selects no eligible sources; check paths, ignores, formats, and size limits")
	}

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

	sort.Strings(paths)
	sort.Strings(ignores)
	summary := &state.report.Config.Duplication
	if relativeRoot, err := filepath.Rel(state.base, root); err == nil {
		summary.ScopeRoot = filepath.ToSlash(relativeRoot)
	}

	summary.Formats, summary.MaxFileBytes = formats, maxBytes
	summary.MinTokens, summary.MinLines, summary.Threshold = config.MinTokens, config.MinLines, config.Threshold
	summary.Mode, summary.Paths, summary.Ignore, summary.Pattern = mode, paths, ignores, pattern
	summary.Gitignore, summary.EnforceThreshold = respectGitignore, config.EnforceThreshold
	summary.DetectorSettings = config.DetectorSettings
	if respectGitignore {
		summary.GitignoreDigest = "sha256:" + digestText(ignoreIdentity.String())
	}

	return config, selected, nil
}

func matchesDuplicationPath(path string, patterns []string) bool {
	for _, pattern := range patterns {
		pattern = strings.TrimSuffix(strings.TrimPrefix(filepath.ToSlash(pattern), "./"), "/")
		if pattern == "." || path == pattern || strings.HasPrefix(path, pattern+"/") {
			return true
		}

		if matched, _ := doublestar.Match(pattern, path); matched {
			return true
		}
	}

	return false
}

func duplicationGitignored(root, path string, cache map[string][]byte) (bool, error) {
	parts := strings.Split(path, "/")
	patterns := make([]gitignore.Pattern, 0)
	for depth := 0; depth < len(parts); depth++ {
		domain := parts[:depth]
		if depth > 0 && gitignore.NewMatcher(patterns).Match(domain, true) {
			return true, nil
		}

		ignorePath := filepath.ToSlash(filepath.Join(strings.Join(domain, "/"), ".gitignore"))
		data, exists := cache[ignorePath]
		if !exists {
			var err error
			data, err = readBoundedFile(filepath.Join(root, filepath.FromSlash(ignorePath)), 1<<20)
			if err != nil && !errors.Is(err, os.ErrNotExist) {
				return false, fmt.Errorf("read duplication gitignore %q: %s", ignorePath, cleanError(err))
			}

			cache[ignorePath] = data
		}

		for _, line := range strings.Split(string(data), "\n") {
			line = strings.TrimRight(line, "\r")
			if strings.TrimSpace(line) == "" || strings.HasPrefix(line, "#") {
				continue
			}

			patterns = append(patterns, gitignore.ParsePattern(line, domain))
		}
	}

	return gitignore.NewMatcher(patterns).Match(parts, false), nil
}
