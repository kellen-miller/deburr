package cli

import (
	"errors"
	"fmt"
	"strconv"
	"strings"

	"github.com/kellen-miller/deburr/internal/analysis"
	"github.com/kellen-miller/deburr/internal/render"
)

const (
	helpFlagLong     = "--help"
	helpFlagShort    = "-h"
	formatFlag       = "--format"
	outputFlag       = "--output"
	ledgerFlag       = "--ledger"
	cpdFlag          = "--cpd"
	maxFileBytesFlag = "--max-file-bytes"
)

type commandOptions struct {
	format render.Format
	output string
	config analysis.Config
	help   bool
}

type cliOption struct {
	name      string
	value     string
	hasValue  bool
	nextIndex int
}

type scannedArguments struct {
	options     []cliOption
	positionals []string
	help        bool
}

func parseOptions(args []string, audit bool) (commandOptions, []string, error) {
	options := commandOptions{format: render.FormatText, config: analysis.DefaultConfig()}
	scanned, err := scanArguments(args, map[string]bool{
		formatFlag:       true,
		outputFlag:       true,
		maxFileBytesFlag: true,
		cpdFlag:          true,
	}, false)
	if err != nil {
		return options, nil, err
	}

	options.help = scanned.help
	for _, option := range scanned.options {
		if err := applyOption(&options, option, audit); err != nil {
			return options, nil, err
		}
	}

	return options, scanned.positionals, nil
}

func scanArguments(args []string, valueFlags map[string]bool, unknownNeedsValue bool) (scannedArguments, error) {
	scanned := scannedArguments{options: make([]cliOption, 0), positionals: make([]string, 0)}
	for index := 0; index < len(args); index++ {
		arg := args[index]
		if arg == "--" {
			scanned.positionals = append(scanned.positionals, args[index+1:]...)
			break
		}

		if arg == helpFlagLong || arg == helpFlagShort {
			scanned.help = true
			continue
		}

		if !strings.HasPrefix(arg, "-") || arg == "-" {
			scanned.positionals = append(scanned.positionals, arg)
			continue
		}

		option, err := readOption(args, index, valueFlags, unknownNeedsValue)
		if err != nil {
			return scanned, err
		}

		scanned.options = append(scanned.options, option)
		index = option.nextIndex
	}

	return scanned, nil
}

func readOption(args []string, index int, valueFlags map[string]bool, unknownNeedsValue bool) (cliOption, error) {
	name, value, hasValue := strings.Cut(args[index], "=")
	option := cliOption{name: name, value: value, hasValue: hasValue, nextIndex: index}
	if (!valueFlags[name] && !unknownNeedsValue) || hasValue {
		return option, nil
	}

	if index+1 >= len(args) {
		return cliOption{}, fmt.Errorf("%s requires a value", name)
	}

	option.value = args[index+1]
	option.hasValue = true
	option.nextIndex++
	return option, nil
}

func applyOption(options *commandOptions, option cliOption, audit bool) error {
	switch option.name {
	case formatFlag:
		format, err := render.ParseFormat(option.value)
		if err != nil {
			return fmt.Errorf("parse --format: %w", err)
		}

		options.format = format
		return nil
	case outputFlag:
		if option.value == "" {
			return errors.New("--output requires a non-empty path")
		}

		options.output = option.value
		return nil
	case "--exclude-tests",
		"--include-testdata",
		"--include-vendor",
		"--include-generated",
		maxFileBytesFlag,
		"--duplicates",
		cpdFlag:
		if !audit {
			return fmt.Errorf("%s is only valid for audit", option.name)
		}

		return applyAuditOption(options, option)
	default:
		return fmt.Errorf("unknown flag %q", option.name)
	}
}

func applyAuditOption(options *commandOptions, option cliOption) error {
	if option.name == maxFileBytesFlag {
		maxBytes, err := strconv.ParseInt(option.value, 10, 64)
		if err != nil || maxBytes <= 0 {
			return fmt.Errorf("--max-file-bytes wants a positive integer, got %q", option.value)
		}

		options.config.MaxFileBytes = maxBytes
		return nil
	}

	if option.name == cpdFlag {
		if option.value == "" {
			return fmt.Errorf("%s requires an executable path", cpdFlag)
		}

		options.config.Duplication.Requested = true
		options.config.Duplication.Tool = option.value
		return nil
	}

	if option.hasValue {
		return fmt.Errorf("%s does not take a value", option.name)
	}

	switch option.name {
	case "--exclude-tests":
		options.config.ExcludeTests = true
	case "--include-testdata":
		options.config.IncludeTestdata = true
	case "--include-vendor":
		options.config.IncludeVendor = true
	case "--include-generated":
		options.config.IncludeGenerated = true
	case "--duplicates":
		options.config.Duplication.Requested = true
	default:
		return fmt.Errorf("unknown audit flag %q", option.name)
	}

	return nil
}
