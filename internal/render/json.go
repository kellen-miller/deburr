package render

import (
	"fmt"
	"io"

	"github.com/kellen-miller/deburr/internal/compare"
	"github.com/kellen-miller/deburr/internal/report"
)

func writeJSON(w io.Writer, value *report.Report) error {
	data, err := MarshalJSON(value)
	if err != nil {
		return err
	}

	if _, err := w.Write(data); err != nil {
		return fmt.Errorf("write JSON report: %w", err)
	}

	return nil
}

func writeComparisonJSON(w io.Writer, value *compare.Result) error {
	data, err := MarshalComparisonJSON(value)
	if err != nil {
		return err
	}

	if _, err := w.Write(data); err != nil {
		return fmt.Errorf("write comparison JSON: %w", err)
	}

	return nil
}
