package analysis

import (
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"path/filepath"
	"slices"
	"strconv"
	"strings"

	"github.com/bmatcuk/doublestar/v4"
)

type duplicationSizeUnit struct {
	suffix     string
	multiplier int64
}

const (
	duplicationConfigMaxBytes   = 1 << 20
	duplicationSizeBase         = 1024
	duplicationFormatGo         = "go"
	duplicationFormatTypeScript = "typescript"
	duplicationFormatTSX        = "tsx"
)

func defaultDuplicationOptions(config DuplicationConfig) duplicationOptions {
	return duplicationOptions{
		config:           config,
		settings:         make(map[string]json.RawMessage),
		formats:          supportedDuplicationFormats(),
		maxBytes:         int64(duplicationFileLimit),
		mode:             "mild",
		respectGitignore: true,
	}
}

func supportedDuplicationFormats() []string {
	return []string{duplicationFormatGo, duplicationFormatTypeScript, duplicationFormatTSX}
}

func readDuplicationOptions(configPath string, options *duplicationOptions) error {
	data, err := readBoundedFile(configPath, duplicationConfigMaxBytes)
	if err != nil {
		return fmt.Errorf("read duplication config: %s", cleanError(err))
	}

	if err := json.Unmarshal(data, &options.settings); err != nil || options.settings == nil {
		return errors.New("duplication config must be a JSON object")
	}

	keys := make([]string, 0, len(options.settings))
	for key := range options.settings {
		keys = append(keys, key)
	}

	slices.Sort(keys)
	for _, key := range keys {
		preserve, err := applyDuplicationSetting(key, options.settings[key], options)
		if err != nil {
			return fmt.Errorf("duplication config %q: %w", key, err)
		}

		if !preserve {
			delete(options.settings, key)
		}
	}

	return nil
}

func applyDuplicationSetting(key string, value json.RawMessage, options *duplicationOptions) (bool, error) {
	switch key {
	case "path":
		return false, decodeDuplicationSetting(key, value, &options.paths)
	case "format":
		err := json.Unmarshal(value, &options.formats)
		if err == nil {
			return false, nil
		}

		var format string
		if err = json.Unmarshal(value, &format); err != nil {
			return false, fmt.Errorf("decode format: %w", err)
		}

		options.formats = strings.Split(format, ",")
		return false, nil
	case "maxSize":
		maxBytes, err := parseDuplicationMaxBytes(value)
		if err == nil {
			options.maxBytes = maxBytes
		}

		return false, err
	case "ignore":
		return false, decodeDuplicationSetting(key, value, &options.ignores)
	case "gitignore":
		return false, decodeDuplicationSetting(key, value, &options.respectGitignore)
	case "pattern":
		return false, decodeDuplicationSetting(key, value, &options.pattern)
	case "minTokens":
		return false, decodeDuplicationSetting(key, value, &options.config.MinTokens)
	case "minLines":
		return false, decodeDuplicationSetting(key, value, &options.config.MinLines)
	case "threshold":
		return false, applyDuplicationThreshold(value, options)
	case "mode":
		return false, decodeDuplicationSetting(key, value, &options.mode)
	case "reporters", "output", "silent", "noColors", "noTips", "absolute", "workers", "exitCode":
		// Deburr owns reporting, bounded execution, and failure policy.
		return false, nil
	case "maxLines":
		return true, validateDuplicationMaximum(value)
	case "skipLocal":
		return false, validateDuplicationSkipLocal(value)
	case "ignoreCase", "ignoreIdentifiers", "ignoreLiterals", "ignoreAnnotations":
		return true, validateDuplicationBoolean(key, value)
	case "ignorePattern":
		var patterns []string
		if err := json.Unmarshal(value, &patterns); err != nil {
			return false, fmt.Errorf("decode ignorePattern: %w", err)
		}

		return true, nil
	default:
		return false, errors.New("unsupported setting")
	}
}

func decodeDuplicationSetting(key string, value json.RawMessage, target any) error {
	if err := json.Unmarshal(value, target); err != nil {
		return fmt.Errorf("decode %s: %w", key, err)
	}

	return nil
}

func applyDuplicationThreshold(value json.RawMessage, options *duplicationOptions) error {
	var threshold float64
	if err := decodeDuplicationSetting("threshold", value, &threshold); err != nil {
		return err
	}
	if threshold < 0 || threshold > 100 {
		return errors.New("must be between 0 and 100")
	}

	options.config.Threshold = strconv.FormatFloat(threshold, 'f', -1, 64)
	return nil
}

func validateDuplicationMaximum(value json.RawMessage) error {
	var maximum int
	if err := decodeDuplicationSetting("maxLines", value, &maximum); err != nil {
		return err
	}
	if maximum <= 0 {
		return errors.New("must be positive")
	}

	return nil
}

func validateDuplicationSkipLocal(value json.RawMessage) error {
	var enabled bool
	if err := decodeDuplicationSetting("skipLocal", value, &enabled); err != nil {
		return err
	}
	if enabled {
		return errors.New("true is unsupported: isolated category snapshots cannot preserve detector path groups")
	}

	return nil
}

func validateDuplicationBoolean(key string, value json.RawMessage) error {
	var enabled bool
	return decodeDuplicationSetting(key, value, &enabled)
}

func parseDuplicationMaxBytes(value json.RawMessage) (int64, error) {
	var maxBytes int64
	if err := json.Unmarshal(value, &maxBytes); err == nil {
		if maxBytes <= 0 {
			return 0, errors.New("must be positive")
		}

		return maxBytes, nil
	}

	var size string
	if err := json.Unmarshal(value, &size); err != nil {
		return 0, fmt.Errorf("decode maxSize as a number or size string: %w", err)
	}

	size = strings.ToLower(strings.TrimSpace(size))
	multiplier := int64(1)
	for _, unit := range []duplicationSizeUnit{
		{suffix: "kb", multiplier: duplicationSizeBase},
		{suffix: "mb", multiplier: duplicationSizeBase * duplicationSizeBase},
		{suffix: "gb", multiplier: duplicationSizeBase * duplicationSizeBase * duplicationSizeBase},
	} {
		if strings.HasSuffix(size, unit.suffix) {
			multiplier = unit.multiplier
			size = strings.TrimSuffix(size, unit.suffix)
			break
		}
	}

	number, err := strconv.ParseInt(strings.TrimSuffix(size, "b"), 10, 64)
	if err != nil {
		return 0, fmt.Errorf("parse maxSize: %w", err)
	}
	if number <= 0 || number > math.MaxInt64/multiplier {
		return 0, errors.New("size must be positive and fit within int64")
	}

	return number * multiplier, nil
}

func normalizeDuplicationOptions(options *duplicationOptions) error {
	if options.config.MinTokens <= 0 || options.config.MinLines <= 0 {
		return errors.New("duplication minimum tokens and lines must be positive")
	}

	if options.mode != "mild" && options.mode != "weak" && options.mode != "strict" {
		return errors.New("duplication mode must be mild, weak, or strict")
	}

	threshold, err := strconv.ParseFloat(strings.TrimSuffix(options.config.Threshold, "%"), 64)
	if err != nil || math.IsNaN(threshold) || math.IsInf(threshold, 0) || threshold < 0 || threshold > 100 {
		return errors.New("duplication threshold must be between 0 and 100")
	}

	options.config.Threshold = strconv.FormatFloat(threshold, 'f', -1, 64)
	globs := slices.Concat(options.paths, options.ignores)
	if options.pattern != "" {
		globs = append(globs, options.pattern)
	}

	for _, glob := range globs {
		if glob == "" || filepath.IsAbs(glob) {
			return errors.New("duplication config paths must be non-empty and relative")
		}
		if !doublestar.ValidatePattern(glob) {
			return fmt.Errorf("duplication config has invalid path pattern %q", glob)
		}
	}

	if len(options.formats) == 0 {
		options.formats = supportedDuplicationFormats()
	}

	formatSet := make(map[string]bool)
	for _, format := range options.formats {
		formatSet[strings.TrimSpace(format)] = true
	}

	formats := make([]string, 0, len(options.formats))
	for _, format := range supportedDuplicationFormats() {
		if formatSet[format] {
			formats = append(formats, format)
		}
	}
	if len(formats) == 0 {
		return errors.New("duplication config selects no supported formats")
	}

	options.formats = formats
	options.config.Formats = strings.Join(formats, ",")
	options.config.Mode = options.mode
	options.config.DetectorSettings, err = json.Marshal(options.settings)
	if err != nil {
		return fmt.Errorf("encode effective duplication settings: %w", err)
	}

	return nil
}
