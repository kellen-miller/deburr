// Package cli implements the deburr command line interface.
package cli

import (
	"context"
	"errors"
	"fmt"
	"io"
	"os"

	"github.com/kellen-miller/deburr/internal/analysis"
	"github.com/kellen-miller/deburr/internal/compare"
	"github.com/kellen-miller/deburr/internal/guide"
	"github.com/kellen-miller/deburr/internal/render"
	"github.com/kellen-miller/deburr/internal/report"
)

const usage = `Usage:
  deburr audit PATH [--format text|json|html|github] [--output FILE]
  deburr compare BEFORE AFTER [--format text|json|html|github] [--output FILE]
  deburr review init REPORT [--output FILE]
  deburr review apply REPORT --ledger FILE [--format text|json|html|github] [--output FILE]
  deburr guide cleanup
  deburr --version

Audit reads Go source without executing it. Findings are advisory; incomplete
analysis and input/output errors return a nonzero status. Reports are written
to stdout unless --output is provided.
`

const auditUsage = `Usage: deburr audit PATH [flags]

Flags:
  --format FORMAT       text, json, html, or github (default text)
  --output FILE         write the report to FILE instead of stdout
  --exclude-tests       exclude _test.go files from analysis
  --include-testdata    analyze Go files under testdata
  --include-vendor      analyze Go files under vendor
  --include-generated   analyze generated Go files
  --max-file-bytes N    exclude source files larger than N bytes
  --duplicates          run the opt-in pinned cpd 5.3.0 adapter
  --cpd PATH            run a cpd executable (must report version 5.3.0)
  --help                show this help
`

const compareUsage = `Usage: deburr compare BEFORE AFTER [flags]

Flags:
  --format FORMAT       text, json, html, or github (default text)
  --output FILE         write the comparison to FILE instead of stdout
  --help                show this help
`

const reviewUsage = `Usage:
  deburr review init REPORT [--output FILE]
  deburr review apply REPORT --ledger FILE [--format text|json|html|github] [--output FILE]

init creates an unreviewed ledger for the report inventory. apply validates
the ledger against the report, preserves missing items as unreviewed, and
marks changed fingerprints stale with the prior decision visible.
`

const (
	version              = analysis.AnalyzerVersion
	exitSuccess          = 0
	exitFailure          = 1
	exitUsage            = 2
	expectedAuditPaths   = 1
	expectedComparePaths = 2
)

// Run executes the CLI and returns its process exit code. It never writes
// diagnostics to stdout.
func Run(ctx context.Context, args []string, stdout, stderr io.Writer) int {
	if stdout == nil {
		stdout = io.Discard
	}

	if stderr == nil {
		stderr = io.Discard
	}

	if len(args) == 0 {
		return exitWithError(stderr, exitUsage, errors.New("missing command; use --help for usage"))
	}

	if args[0] == "--version" || args[0] == "-v" || args[0] == "version" {
		if len(args) != 1 {
			return exitWithError(stderr, exitUsage, errors.New("version does not accept additional arguments"))
		}

		if _, err := fmt.Fprintf(stdout, "deburr %s\n", version); err != nil {
			return exitWithError(stderr, exitFailure, fmt.Errorf("write version: %w", err))
		}

		return exitSuccess
	}

	if args[0] == helpFlagLong || args[0] == helpFlagShort || args[0] == "help" {
		if len(args) != 1 {
			return exitWithError(stderr, exitUsage, errors.New("help does not accept additional arguments"))
		}

		if _, err := io.WriteString(stdout, usage); err != nil {
			return exitWithError(stderr, exitFailure, fmt.Errorf("write help: %w", err))
		}

		return exitSuccess
	}

	switch args[0] {
	case "audit":
		return runAudit(ctx, args[1:], stdout, stderr)
	case "compare":
		return runCompare(args[1:], stdout, stderr)
	case "review":
		return runReview(args[1:], stdout, stderr)
	case "guide":
		return runGuide(args[1:], stdout, stderr)
	default:
		return exitWithError(stderr, exitUsage, fmt.Errorf("unknown command %q; use --help for usage", args[0]))
	}
}

func runAudit(ctx context.Context, args []string, stdout, stderr io.Writer) int {
	options, positionals, err := parseOptions(args, true)
	if err != nil {
		return exitWithError(stderr, exitUsage, err)
	}

	if options.help {
		if len(positionals) != 0 {
			return exitWithError(stderr, exitUsage, errors.New("audit --help does not accept additional arguments"))
		}

		if _, err := io.WriteString(stdout, auditUsage); err != nil {
			return exitWithError(stderr, exitFailure, fmt.Errorf("write audit help: %w", err))
		}

		return exitSuccess
	}

	if len(positionals) != expectedAuditPaths {
		return exitWithError(stderr, exitUsage, fmt.Errorf("audit wants exactly one PATH, got %d", len(positionals)))
	}

	target := positionals[0]
	if options.output != "" {
		if err := validateOutputPath(target, options.output); err != nil {
			return exitWithError(stderr, exitUsage, err)
		}
	}

	value, analysisErr := analysis.Analyze(ctx, target, options.config)
	applyProvenance(&value, target)
	report.InitializeReview(&value)
	if err := writeAuditReport(&value, target, &options, stdout); err != nil {
		return exitWithError(stderr, exitFailure, err)
	}

	if analysisErr != nil {
		return exitWithError(stderr, exitFailure, analysisErr)
	}

	return exitSuccess
}

func runCompare(args []string, stdout, stderr io.Writer) int {
	options, positionals, err := parseOptions(args, false)
	if err != nil {
		return exitWithError(stderr, exitUsage, err)
	}

	if options.help {
		if len(positionals) != 0 {
			return exitWithError(stderr, exitUsage, errors.New("compare --help does not accept additional arguments"))
		}

		if _, err := io.WriteString(stdout, compareUsage); err != nil {
			return exitWithError(stderr, exitFailure, fmt.Errorf("write compare help: %w", err))
		}

		return exitSuccess
	}

	if len(positionals) != expectedComparePaths {
		return exitWithError(
			stderr,
			exitUsage,
			fmt.Errorf("compare wants exactly BEFORE and AFTER reports, got %d", len(positionals)),
		)
	}

	if options.output != "" {
		if err := validateCompareOutputPath(options.output, positionals[0], positionals[1]); err != nil {
			return exitWithError(stderr, exitUsage, err)
		}
	}

	before, after, err := readComparisonReports(positionals[0], positionals[1])
	if err != nil {
		return exitWithError(stderr, exitFailure, err)
	}

	result, err := compare.Compare(&before, &after)
	if err != nil {
		return exitWithError(stderr, exitFailure, err)
	}

	if err := writeComparisonResult(&result, &options, stdout); err != nil {
		return exitWithError(stderr, exitFailure, err)
	}

	if !result.Compatible || !result.Complete {
		return exitWithError(
			stderr,
			exitFailure,
			errors.New("reports are incompatible or incomplete; no improvement conclusion was made"),
		)
	}

	return exitSuccess
}

func runGuide(args []string, stdout, stderr io.Writer) int {
	if len(args) == 1 && (args[0] == helpFlagLong || args[0] == helpFlagShort) {
		if _, err := io.WriteString(stdout, "Usage: deburr guide cleanup\n"); err != nil {
			return exitWithError(stderr, exitFailure, fmt.Errorf("write guide help: %w", err))
		}

		return exitSuccess
	}

	if len(args) != 1 || args[0] != "cleanup" {
		return exitWithError(stderr, exitUsage, errors.New("guide wants cleanup"))
	}

	if _, err := io.WriteString(stdout, guide.Cleanup()); err != nil {
		return exitWithError(stderr, exitFailure, fmt.Errorf("write cleanup guide: %w", err))
	}

	return exitSuccess
}

func readComparisonReports(beforePath, afterPath string) (report.Report, report.Report, error) {
	before, err := readComparisonReport(beforePath, "before")
	if err != nil {
		return report.Report{}, report.Report{}, err
	}

	after, err := readComparisonReport(afterPath, "after")
	if err != nil {
		return report.Report{}, report.Report{}, err
	}

	return before, after, nil
}

func readComparisonReport(path, label string) (report.Report, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return report.Report{}, fmt.Errorf("read %s report %q: %w", label, path, err)
	}

	value, err := render.DecodeJSON(data)
	if err != nil {
		return report.Report{}, fmt.Errorf("%s report %q: %w", label, path, err)
	}

	return value, nil
}

func writeComparisonResult(result *compare.Result, options *commandOptions, stdout io.Writer) error {
	if options.output != "" {
		return writeOutputFile(options.output, func(w io.Writer) error {
			if err := render.WriteComparison(w, result, options.format); err != nil {
				return fmt.Errorf("write comparison: %w", err)
			}

			return nil
		})
	}

	if err := render.WriteComparison(stdout, result, options.format); err != nil {
		return fmt.Errorf("write comparison: %w", err)
	}

	return nil
}
