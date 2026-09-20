package render

import (
	"fmt"
	"io"
	"strings"

	"github.com/kellen-miller/deburr/internal/compare"
	"github.com/kellen-miller/deburr/internal/report"
)

func writeComparisonText(w io.Writer, value *compare.Result) error {
	if err := ValidateComparison(value); err != nil {
		return err
	}

	var out strings.Builder
	writeComparisonHeader(&out, value)
	writeComparisonReview(&out, value)
	writeComparisonFindings(&out, value)
	writeComparisonFunctions(&out, value)
	writeComparisonClones(&out, value)
	writeComparisonDuplication(&out, value)

	if _, err := io.WriteString(w, out.String()); err != nil {
		return fmt.Errorf("write comparison text: %w", err)
	}

	return nil
}

func writeComparisonHeader(out *strings.Builder, value *compare.Result) {
	fmt.Fprintf(out, "Deburr comparison (schema %s)\n", value.SchemaVersion)
	fmt.Fprintf(
		out,
		"Analyzer: %s %s -> %s %s\n",
		value.BeforeAnalyzer.ID,
		value.BeforeAnalyzer.Version,
		value.AfterAnalyzer.ID,
		value.AfterAnalyzer.Version,
	)
	writeTextProvenance(out, value.BeforeProvenance, "Before provenance")
	writeTextProvenance(out, value.AfterProvenance, "After provenance")
	fmt.Fprintf(out, "Compatible: %t\n", value.Compatible)
	fmt.Fprintf(out, "Complete: %t\n", value.Complete)
	if !value.Compatible || !value.Complete {
		out.WriteString("Change conclusion: unavailable; no finding, function, or clone improvement claim was made.\n")
	}
	if value.Reason != "" {
		fmt.Fprintf(out, "Reason: %s\n", value.Reason)
	}

	fmt.Fprintf(
		out,
		"Coverage: before analyzed=%d read_errors=%d parse_errors=%d; after analyzed=%d read_errors=%d parse_errors=%d\n",
		value.BeforeCoverage.Analyzed,
		value.BeforeCoverage.ReadErrors,
		value.BeforeCoverage.ParseErrors,
		value.AfterCoverage.Analyzed,
		value.AfterCoverage.ReadErrors,
		value.AfterCoverage.ParseErrors,
	)
	if value.MetricsDelta == nil {
		out.WriteString("Metrics delta: unavailable\n")
	} else {
		writeMetricDelta(out, "All", value.MetricsDelta.All)
		writeMetricDelta(out, "Production", value.MetricsDelta.Production)
		writeMetricDelta(out, "Test", value.MetricsDelta.Test)
	}
}

func writeComparisonFindings(out *strings.Builder, value *compare.Result) {
	newCount := 0
	noLongerReportedCount := 0
	for index := range value.FindingChanges {
		switch value.FindingChanges[index].State {
		case compare.ComparisonNew:
			newCount++
		case compare.ComparisonNoLongerReported, compare.ComparisonSourceRemoved:
			noLongerReportedCount++
		default:
			continue
		}
	}
	fmt.Fprintf(out, "New findings (%d)\n", newCount)
	fmt.Fprintf(out, "No longer reported findings (%d)\n", noLongerReportedCount)
	fmt.Fprintf(out, "Finding changes (%d)\n", len(value.FindingChanges))
	for index := range value.FindingChanges {
		writeFindingChange(out, &value.FindingChanges[index])
	}

	if len(value.Ambiguous) > 0 {
		fmt.Fprintf(out, "Ambiguous finding matches (%d)\n", len(value.Ambiguous))
		for index := range value.Ambiguous {
			finding := value.Ambiguous[index]
			fmt.Fprintf(
				out,
				"- %s:%s: %s before=%s after=%s\n",
				finding.Path,
				finding.Rule,
				finding.Message,
				strings.Join(finding.BeforeIDs, ","),
				strings.Join(finding.AfterIDs, ","),
			)
		}
	}
}

func writeComparisonReview(out *strings.Builder, value *compare.Result) {
	fmt.Fprintf(
		out,
		"Review before: total=%d unreviewed=%d refactored=%d retained=%d deferred=%d stale=%d\n",
		value.BeforeReview.Counts.Total,
		value.BeforeReview.Counts.Unreviewed,
		value.BeforeReview.Counts.Refactored,
		value.BeforeReview.Counts.Retained,
		value.BeforeReview.Counts.Deferred,
		value.BeforeReview.Counts.Stale,
	)
	fmt.Fprintf(
		out,
		"Review after: total=%d unreviewed=%d refactored=%d retained=%d deferred=%d stale=%d\n",
		value.AfterReview.Counts.Total,
		value.AfterReview.Counts.Unreviewed,
		value.AfterReview.Counts.Refactored,
		value.AfterReview.Counts.Retained,
		value.AfterReview.Counts.Deferred,
		value.AfterReview.Counts.Stale,
	)
}

func writeFindingChange(out *strings.Builder, change *compare.FindingComparison) {
	before, after := "", ""
	if change.Before != nil {
		before = formatFindingComparison(change.Before)
	}
	if change.After != nil {
		after = formatFindingComparison(change.After)
	}
	writeComparisonItemLine(
		out,
		change.State,
		change.MatchBasis,
		before,
		after,
		change.BeforeIDs,
		change.AfterIDs,
		change.BeforeReview,
		change.AfterReview,
	)
}

func formatFindingComparison(value *report.Finding) string {
	return fmt.Sprintf(
		"%s [%s] %s: %s (id=%s ambiguous=%t)",
		formatReportLocation(value.Path, value.Start, value.End),
		value.Severity,
		value.Rule,
		value.Message,
		value.ID,
		value.IdentityAmbiguous,
	)
}

func writeComparisonFunctions(out *strings.Builder, value *compare.Result) {
	fmt.Fprintf(out, "Function changes (%d)\n", len(value.FunctionChanges))
	for index := range value.FunctionChanges {
		change := &value.FunctionChanges[index]
		before, after := "", ""
		if change.Before != nil {
			before = formatFunctionComparison(change.Before)
		}
		if change.After != nil {
			after = formatFunctionComparison(change.After)
		}
		writeComparisonItemLine(
			out,
			change.State,
			change.MatchBasis,
			before,
			after,
			change.BeforeIDs,
			change.AfterIDs,
			change.BeforeReview,
			change.AfterReview,
		)
		if change.State == compare.ComparisonResolvedThreshold ||
			change.State == compare.ComparisonDeletedFunction ||
			change.State == compare.ComparisonSourceRemoved {
			fmt.Fprint(out, "  note=no_behavior_conclusion\n")
		}
	}
}

func writeComparisonClones(out *strings.Builder, value *compare.Result) {
	fmt.Fprintf(out, "Clone changes (%d)\n", len(value.CloneChanges))
	for index := range value.CloneChanges {
		change := &value.CloneChanges[index]
		before, after := "", ""
		if change.Before != nil {
			before = formatCloneComparison(change.Before)
		}
		if change.After != nil {
			after = formatCloneComparison(change.After)
		}
		writeComparisonItemLine(
			out,
			change.State,
			change.MatchBasis,
			before,
			after,
			change.BeforeIDs,
			change.AfterIDs,
			change.BeforeReview,
			change.AfterReview,
		)
	}
}

func writeComparisonItemLine(
	out *strings.Builder,
	state compare.ComparisonState,
	basis compare.MatchBasis,
	before, after string,
	beforeIDs, afterIDs []string,
	beforeReview, afterReview *report.ReviewItem,
) {
	fmt.Fprintf(out, "- %s", state)
	if basis != "" {
		fmt.Fprintf(out, " match=%s", basis)
	}
	if before != "" {
		fmt.Fprintf(out, " before=%s", before)
	}
	if after != "" {
		fmt.Fprintf(out, " after=%s", after)
	}
	if len(beforeIDs) > 0 || len(afterIDs) > 0 {
		fmt.Fprintf(out, " before_ids=%s after_ids=%s", strings.Join(beforeIDs, ","), strings.Join(afterIDs, ","))
	}
	writeComparisonReviewDecision(out, beforeReview, "before")
	writeComparisonReviewDecision(out, afterReview, "after")
	out.WriteByte('\n')
}

func writeComparisonReviewDecision(out *strings.Builder, item *report.ReviewItem, side string) {
	if item == nil {
		return
	}
	fmt.Fprintf(out, " %s_review=%s", side, item.Status)
	if item.Stale {
		fmt.Fprint(out, " stale=true")
	}
	if item.ReasonCategory != "" {
		fmt.Fprintf(out, " reason_category=%s", item.ReasonCategory)
	}
	if item.Reason != "" {
		fmt.Fprintf(out, " reason=%s", item.Reason)
	}
	if item.Evidence != "" {
		fmt.Fprintf(out, " evidence=%s", item.Evidence)
	}
	if item.Upstream != "" {
		fmt.Fprintf(out, " upstream=%s", item.Upstream)
	}
	if item.Previous != nil {
		fmt.Fprintf(out, " previous=%s", formatAuditHTMLPrevious(item.Previous))
	}
}

func formatFunctionComparison(value *report.Function) string {
	return fmt.Sprintf(
		"%s name=%s sloc=%d cyclomatic=%d nesting=%d mass=%.6f high_complexity=%t flat_guards=%d field_mappings=%d nested_branches=%d assertion_like_calls=%d ambiguous=%t",
		formatReportLocation(value.Path, value.Start, value.End),
		value.Name,
		value.SLOC,
		value.Cyclomatic,
		value.MaxNesting,
		value.Mass,
		value.HighComplexity,
		value.StructuralSignals.FlatGuards,
		value.StructuralSignals.FieldMappings,
		value.StructuralSignals.NestedBranches,
		value.StructuralSignals.AssertionLikeCalls,
		value.IdentityAmbiguous,
	)
}

func formatCloneComparison(value *report.Clone) string {
	locations := make([]string, 0, len(value.Locations))
	for index := range value.Locations {
		location := value.Locations[index]
		locations = append(locations, formatReportLocation(location.Path, location.Start, location.End))
	}
	return fmt.Sprintf(
		"id=%s family=%s tokens=%d lines=%d ambiguous=%t locations=%s",
		value.ID,
		value.FamilyID,
		value.Tokens,
		value.Lines,
		value.IdentityAmbiguous,
		strings.Join(locations, ";"),
	)
}

func writeComparisonDuplication(out *strings.Builder, value *compare.Result) {
	fmt.Fprintf(
		out,
		"Duplication: before=%s after=%s comparable=%t before_config=%s/%s/%s/%d/%d/%s after_config=%s/%s/%s/%d/%d/%s",
		value.Duplication.BeforeStatus,
		value.Duplication.AfterStatus,
		value.Duplication.Comparable,
		value.Duplication.BeforeConfig.Tool,
		value.Duplication.BeforeConfig.Version,
		value.Duplication.BeforeConfig.Scope,
		value.Duplication.BeforeConfig.MinTokens,
		value.Duplication.BeforeConfig.MinLines,
		value.Duplication.BeforeConfig.Threshold,
		value.Duplication.AfterConfig.Tool,
		value.Duplication.AfterConfig.Version,
		value.Duplication.AfterConfig.Scope,
		value.Duplication.AfterConfig.MinTokens,
		value.Duplication.AfterConfig.MinLines,
		value.Duplication.AfterConfig.Threshold,
	)
	if value.Duplication.Reason != "" {
		fmt.Fprintf(out, " reason=%s", value.Duplication.Reason)
	}

	out.WriteByte('\n')
}

func writeMetricDelta(out *strings.Builder, name string, delta compare.MetricDelta) {
	fmt.Fprintf(
		out,
		"%s metric delta: code_lines=%d functions=%d cyclomatic=%d max_nesting=%d mass=%+.6f high_complexity_functions=%d high_complexity_mass=%+.6f",
		name,
		delta.CodeLines,
		delta.Functions,
		delta.Cyclomatic,
		delta.MaxNesting,
		delta.Mass,
		delta.HighComplexityFunctions,
		delta.HighComplexityMass,
	)
	if delta.HighComplexityMassShare == nil {
		out.WriteString(" high_complexity_mass_share=unavailable")
	} else {
		fmt.Fprintf(out, " high_complexity_mass_share=%+.6f", *delta.HighComplexityMassShare)
	}

	out.WriteByte('\n')
}
