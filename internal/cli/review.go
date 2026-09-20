package cli

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"

	"github.com/kellen-miller/deburr/internal/render"
	"github.com/kellen-miller/deburr/internal/report"
)

type reviewOptions struct {
	format render.Format
	output string
	ledger string
	help   bool
}

func runReview(args []string, stdout, stderr io.Writer) int {
	if len(args) == 0 || args[0] == helpFlagLong || args[0] == helpFlagShort {
		if len(args) > 1 {
			return exitWithError(stderr, exitUsage, errors.New("review --help does not accept additional arguments"))
		}

		if _, err := io.WriteString(stdout, reviewUsage); err != nil {
			return exitWithError(stderr, exitFailure, fmt.Errorf("write review help: %w", err))
		}

		return exitSuccess
	}

	switch args[0] {
	case "init":
		return runReviewInit(args[1:], stdout, stderr)
	case "apply":
		return runReviewApply(args[1:], stdout, stderr)
	default:
		return exitWithError(stderr, exitUsage, fmt.Errorf("review wants init or apply, got %q", args[0]))
	}
}

func runReviewInit(args []string, stdout, stderr io.Writer) int {
	options, positionals, err := parseReviewOptions(args, false)
	if err != nil {
		return exitWithError(stderr, exitUsage, err)
	}

	if options.help {
		if len(positionals) != 0 {
			return exitWithError(
				stderr,
				exitUsage,
				errors.New("review init --help does not accept additional arguments"),
			)
		}

		if _, err := io.WriteString(stdout, reviewUsage); err != nil {
			return exitWithError(stderr, exitFailure, fmt.Errorf("write review help: %w", err))
		}

		return exitSuccess
	}

	if len(positionals) != 1 {
		return exitWithError(
			stderr,
			exitUsage,
			fmt.Errorf("review init wants exactly one REPORT, got %d", len(positionals)),
		)
	}

	if options.output != "" {
		if err := validateCompareOutputPath(options.output, positionals[0]); err != nil {
			return exitWithError(stderr, exitUsage, err)
		}
	}

	value, err := readComparisonReport(positionals[0], "review")
	if err != nil {
		return exitWithError(stderr, exitFailure, err)
	}

	ledger := report.InitializeReview(&value)
	if err := writeReviewLedger(&ledger, options.output, stdout); err != nil {
		return exitWithError(stderr, exitFailure, err)
	}

	return exitSuccess
}

func runReviewApply(args []string, stdout, stderr io.Writer) int {
	options, positionals, err := parseReviewOptions(args, true)
	if err != nil {
		return exitWithError(stderr, exitUsage, err)
	}

	if options.help {
		if len(positionals) != 0 {
			return exitWithError(
				stderr,
				exitUsage,
				errors.New("review apply --help does not accept additional arguments"),
			)
		}

		if _, err := io.WriteString(stdout, reviewUsage); err != nil {
			return exitWithError(stderr, exitFailure, fmt.Errorf("write review help: %w", err))
		}

		return exitSuccess
	}

	if len(positionals) != 1 {
		return exitWithError(
			stderr,
			exitUsage,
			fmt.Errorf("review apply wants exactly one REPORT, got %d", len(positionals)),
		)
	}

	if options.ledger == "" {
		return exitWithError(stderr, exitUsage, errors.New("review apply requires --ledger"))
	}

	if options.output != "" {
		if err := validateCompareOutputPath(options.output, positionals[0], options.ledger); err != nil {
			return exitWithError(stderr, exitUsage, err)
		}
	}

	value, err := readComparisonReport(positionals[0], "review")
	if err != nil {
		return exitWithError(stderr, exitFailure, err)
	}

	ledger, err := readReviewLedger(options.ledger)
	if err != nil {
		return exitWithError(stderr, exitFailure, err)
	}

	if err := report.ApplyReview(&value, &ledger); err != nil {
		return exitWithError(stderr, exitFailure, fmt.Errorf("apply review ledger: %w", err))
	}

	if err := writeReviewedReport(&value, options, stdout); err != nil {
		return exitWithError(stderr, exitFailure, err)
	}

	return exitSuccess
}

func parseReviewOptions(args []string, apply bool) (reviewOptions, []string, error) {
	options := reviewOptions{format: render.FormatText}
	scanned, err := scanArguments(args, map[string]bool{
		formatFlag: true,
		outputFlag: true,
		ledgerFlag: true,
	}, true)
	if err != nil {
		return options, nil, err
	}

	options.help = scanned.help
	for _, option := range scanned.options {
		if err := applyReviewOption(&options, option, apply); err != nil {
			return options, nil, err
		}
	}

	return options, scanned.positionals, nil
}

func applyReviewOption(options *reviewOptions, option cliOption, apply bool) error {
	switch option.name {
	case formatFlag:
		if !apply {
			return errors.New("review init does not accept --format")
		}

		format, err := render.ParseFormat(option.value)
		if err != nil {
			return fmt.Errorf("parse --format: %w", err)
		}

		options.format = format
	case outputFlag:
		if option.value == "" {
			return errors.New("--output requires a non-empty path")
		}

		options.output = option.value
	case ledgerFlag:
		if !apply {
			return errors.New("review init does not accept --ledger")
		}

		if option.value == "" {
			return errors.New("--ledger requires a non-empty path")
		}

		options.ledger = option.value
	default:
		return fmt.Errorf("unknown review flag %q", option.name)
	}

	return nil
}

func readReviewLedger(path string) (report.ReviewLedger, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return report.ReviewLedger{}, fmt.Errorf("read review ledger %q: %w", path, err)
	}

	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.DisallowUnknownFields()
	var ledger report.ReviewLedger
	if err := decoder.Decode(&ledger); err != nil {
		return report.ReviewLedger{}, fmt.Errorf("decode review ledger %q: %w", path, err)
	}

	var extra any
	if err := decoder.Decode(&extra); err != io.EOF {
		if err == nil {
			return report.ReviewLedger{}, fmt.Errorf("decode review ledger %q: multiple values", path)
		}

		return report.ReviewLedger{}, fmt.Errorf("decode review ledger %q: %w", path, err)
	}

	return ledger, nil
}

func writeReviewLedger(ledger *report.ReviewLedger, output string, stdout io.Writer) error {
	write := func(w io.Writer) error {
		data, err := json.MarshalIndent(ledger, "", "  ")
		if err != nil {
			return fmt.Errorf("marshal review ledger: %w", err)
		}

		data = append(data, '\n')
		if _, err := w.Write(data); err != nil {
			return fmt.Errorf("write review ledger: %w", err)
		}

		return nil
	}

	if output != "" {
		return writeOutputFile(output, write)
	}

	return write(stdout)
}

func writeReviewedReport(value *report.Report, options reviewOptions, stdout io.Writer) error {
	write := func(w io.Writer) error {
		if err := render.Write(w, value, options.format); err != nil {
			return fmt.Errorf("write reviewed report: %w", err)
		}

		return nil
	}

	if options.output != "" {
		return writeOutputFile(options.output, write)
	}

	return write(stdout)
}
