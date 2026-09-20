// Package render turns a report into a stable human- or machine-readable
// representation.
package render

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"strings"

	"github.com/kellen-miller/deburr/internal/compare"
	"github.com/kellen-miller/deburr/internal/report"
)

type Format string

const (
	FormatText   Format = "text"
	FormatJSON   Format = "json"
	FormatHTML   Format = "html"
	FormatGitHub Format = "github"
)

func ParseFormat(value string) (Format, error) {
	format := Format(strings.ToLower(strings.TrimSpace(value)))
	switch format {
	case FormatText, FormatJSON, FormatHTML, FormatGitHub:
		return format, nil
	default:
		return "", fmt.Errorf("unsupported format %q (want text, json, html, or github)", value)
	}
}

func Write(w io.Writer, value *report.Report, format Format) error {
	switch format {
	case FormatText:
		return writeText(w, value)
	case FormatJSON:
		return writeJSON(w, value)
	case FormatHTML:
		return writeHTML(w, value)
	case FormatGitHub:
		return writeGitHub(w, value)
	default:
		return fmt.Errorf("unsupported format %q", format)
	}
}

func WriteComparison(w io.Writer, value *compare.Result, format Format) error {
	switch format {
	case FormatText:
		return writeComparisonText(w, value)
	case FormatJSON:
		return writeComparisonJSON(w, value)
	case FormatHTML:
		return writeComparisonHTML(w, value)
	case FormatGitHub:
		return writeComparisonGitHub(w, value)
	default:
		return fmt.Errorf("unsupported format %q", format)
	}
}

func MarshalJSON(value *report.Report) ([]byte, error) {
	if err := Validate(value); err != nil {
		return nil, err
	}

	data, err := json.MarshalIndent(value, "", "  ")
	if err != nil {
		return nil, fmt.Errorf("marshal report: %w", err)
	}

	return append(data, '\n'), nil
}

func DecodeJSON(data []byte) (report.Report, error) {
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.DisallowUnknownFields()
	var value report.Report
	if err := decoder.Decode(&value); err != nil {
		return report.Report{}, fmt.Errorf("decode report JSON: %w", err)
	}

	var extra any
	if err := decoder.Decode(&extra); err != io.EOF {
		if err == nil {
			return report.Report{}, errors.New("decode report JSON: multiple values")
		}

		return report.Report{}, fmt.Errorf("decode report JSON: %w", err)
	}

	if err := Validate(&value); err != nil {
		return report.Report{}, err
	}

	return value, nil
}

func Validate(value *report.Report) error {
	if value == nil {
		return errors.New("report is nil")
	}

	if value.SchemaVersion == "" {
		return errors.New("report schema_version is required")
	}

	if value.SchemaVersion != report.SchemaVersion {
		return fmt.Errorf("unsupported report schema_version %q", value.SchemaVersion)
	}

	if value.Analyzer.ID == "" || value.Analyzer.Version == "" {
		return errors.New("report analyzer id and version are required")
	}

	if value.Config.Language == "" {
		return errors.New("report config language is required")
	}

	if len(value.Scope.Languages) == 0 {
		return errors.New("report scope languages are required")
	}

	return nil
}

func reviewReportForRender(value *report.Report) (report.Report, error) {
	prepared := *value
	if value.Review == nil {
		report.InitializeReview(&prepared)
		return prepared, nil
	}

	ledger := *value.Review
	prepared.Review = nil
	if err := report.ApplyReview(&prepared, &ledger); err != nil {
		return report.Report{}, fmt.Errorf("validate review ledger: %w", err)
	}

	return prepared, nil
}

func MarshalComparisonJSON(value *compare.Result) ([]byte, error) {
	if err := ValidateComparison(value); err != nil {
		return nil, err
	}

	data, err := json.MarshalIndent(value, "", "  ")
	if err != nil {
		return nil, fmt.Errorf("marshal comparison: %w", err)
	}

	return append(data, '\n'), nil
}

func DecodeComparisonJSON(data []byte) (compare.Result, error) {
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.DisallowUnknownFields()
	var value compare.Result
	if err := decoder.Decode(&value); err != nil {
		return compare.Result{}, fmt.Errorf("decode comparison JSON: %w", err)
	}

	var extra any
	if err := decoder.Decode(&extra); err != io.EOF {
		if err == nil {
			return compare.Result{}, errors.New("decode comparison JSON: multiple values")
		}

		return compare.Result{}, fmt.Errorf("decode comparison JSON: %w", err)
	}

	if err := ValidateComparison(&value); err != nil {
		return compare.Result{}, err
	}

	return value, nil
}

func ValidateComparison(value *compare.Result) error {
	if value == nil {
		return errors.New("comparison is nil")
	}

	if value.SchemaVersion != compare.SchemaVersion {
		return fmt.Errorf("unsupported comparison schema_version %q", value.SchemaVersion)
	}

	if value.BeforeAnalyzer.ID == "" || value.BeforeAnalyzer.Version == "" || value.AfterAnalyzer.ID == "" ||
		value.AfterAnalyzer.Version == "" {
		return errors.New("comparison analyzer identities are required")
	}

	return nil
}
