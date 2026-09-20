package render

import (
	"fmt"
	"io"
	"sort"
	"strings"

	"github.com/kellen-miller/deburr/internal/report"
)

const hotspotLimit = 20

func writeText(w io.Writer, value *report.Report) error {
	if err := Validate(value); err != nil {
		return err
	}

	var out strings.Builder
	writeTextHeader(&out, value)
	writeTextFailures(&out, value)
	writeTextFunctions(&out, value)
	writeTextFindings(&out, value)
	writeTextDuplication(&out, value)
	if err := writeTextReview(&out, value); err != nil {
		return err
	}
	if _, err := io.WriteString(w, out.String()); err != nil {
		return fmt.Errorf("write text report: %w", err)
	}

	return nil
}

func writeTextHeader(out *strings.Builder, value *report.Report) {
	fmt.Fprintf(out, "Deburr report (schema %s)\n", value.SchemaVersion)
	fmt.Fprintf(out, "Analyzer: %s %s\n", value.Analyzer.ID, value.Analyzer.Version)
	writeTextProvenance(out, value.Provenance, "Provenance")
	fmt.Fprintf(out, "Language: %s\n", value.Config.Language)
	fmt.Fprintf(out, "Scope: %s\n", strings.Join(value.Scope.Languages, ","))
	fmt.Fprintf(
		out,
		"Coverage: discovered=%d analyzed=%d excluded=%d unsupported=%d read_errors=%d parse_errors=%d symlinks=%d excluded_directories=%d\n",
		value.Coverage.Discovered,
		value.Coverage.Analyzed,
		value.Coverage.Excluded,
		value.Coverage.Unsupported,
		value.Coverage.ReadErrors,
		value.Coverage.ParseErrors,
		value.Coverage.Symlinks,
		value.Coverage.ExcludedDirectories,
	)
	if value.Coverage.Analyzed == 0 {
		out.WriteString("Analysis: no analyzable Go files\n")
	}

	writeMetricBucket(out, "All", value.Metrics.All)
	writeMetricBucket(out, "Production", value.Metrics.Production)
	writeMetricBucket(out, "Test", value.Metrics.Test)

	fmt.Fprintf(out, "Files: %d discovered entries; use JSON/HTML for the full inventory\n", len(value.Files))
}

func writeTextFailures(out *strings.Builder, value *report.Report) {
	failures := make([]report.File, 0)
	for index := range value.Files {
		file := value.Files[index]
		if file.Status == report.FileReadError || file.Status == report.FileParseError {
			failures = append(failures, file)
		}
	}

	if len(failures) == 0 {
		return
	}

	fmt.Fprintf(out, "Source failures (%d)\n", len(failures))
	for index := range failures {
		file := failures[index]
		message := file.Error
		if message == "" {
			message = file.Reason
		}

		fmt.Fprintf(out, "- %s [%s] %s\n", file.Path, file.Status, message)
	}
}

func writeTextFunctions(out *strings.Builder, value *report.Report) {
	fmt.Fprintf(out, "\nFunction hotspots (ranked by raw mass; JSON/HTML include all %d)\n", len(value.Functions))
	buckets := splitFunctions(value.Functions)
	writeFunctionHotspots(out, "Production", buckets.production)
	writeFunctionHotspots(out, "Test", buckets.test)
	if len(buckets.other) > 0 {
		writeFunctionHotspots(out, "Other", buckets.other)
	}
}

type functionBuckets struct {
	production []report.Function
	test       []report.Function
	other      []report.Function
}

func splitFunctions(functions []report.Function) functionBuckets {
	buckets := functionBuckets{
		production: make([]report.Function, 0),
		test:       make([]report.Function, 0),
		other:      make([]report.Function, 0),
	}
	for index := range functions {
		function := functions[index]
		switch function.Category {
		case report.CategoryProduction:
			buckets.production = append(buckets.production, function)
		case report.CategoryTest:
			buckets.test = append(buckets.test, function)
		default:
			buckets.other = append(buckets.other, function)
		}
	}

	return buckets
}

func writeTextFindings(out *strings.Builder, value *report.Report) {
	fmt.Fprintf(out, "\nFindings (%d)\n", len(value.Findings))
	for index := range value.Findings {
		finding := &value.Findings[index]
		fmt.Fprintf(
			out,
			"- %s:%d:%d-%d:%d [%s] %s: %s identity_ambiguous=%t",
			finding.Path,
			finding.Start.Line,
			finding.Start.Column,
			finding.End.Line,
			finding.End.Column,
			finding.Severity,
			finding.Rule,
			finding.Message,
			finding.IdentityAmbiguous,
		)
		if len(finding.Details) > 0 {
			out.WriteString(" (")
			for detailIndex := range finding.Details {
				if detailIndex > 0 {
					out.WriteString(", ")
				}

				detail := finding.Details[detailIndex]
				fmt.Fprintf(out, "%s=%s", detail.Key, detail.Value)
			}

			out.WriteByte(')')
		}

		out.WriteByte('\n')
	}
}

func writeTextDuplication(out *strings.Builder, value *report.Report) {
	if value.Duplication == nil {
		out.WriteString("\nDuplication: not_requested\n")
		return
	}

	config := value.Duplication.Config
	fmt.Fprintf(
		out,
		"\nDuplication: %s tool=%s version=%s scope=%s min_tokens=%d min_lines=%d threshold=%s",
		value.Duplication.Status,
		config.Tool,
		config.Version,
		config.Scope,
		config.MinTokens,
		config.MinLines,
		config.Threshold,
	)
	if value.Duplication.Error != "" {
		fmt.Fprintf(out, " error=%s", value.Duplication.Error)
	}

	out.WriteByte('\n')
	for cloneIndex := range value.Duplication.Clones {
		clone := &value.Duplication.Clones[cloneIndex]
		fmt.Fprintf(
			out,
			"- clone %s family=%s tokens=%d lines=%d ambiguous=%t\n",
			clone.ID,
			clone.FamilyID,
			clone.Tokens,
			clone.Lines,
			clone.IdentityAmbiguous,
		)
		for locationIndex := range clone.Locations {
			location := clone.Locations[locationIndex]
			fmt.Fprintf(
				out,
				"  - %s:%d:%d-%d:%d\n",
				location.Path,
				location.Start.Line,
				location.Start.Column,
				location.End.Line,
				location.End.Column,
			)
		}
	}
}

func writeTextReview(out *strings.Builder, value *report.Report) error {
	prepared, err := reviewReportForRender(value)
	if err != nil {
		return fmt.Errorf("prepare review output: %w", err)
	}

	ledger := prepared.Review
	fmt.Fprintf(
		out,
		"\nReview: total=%d unreviewed=%d refactored=%d retained=%d deferred=%d stale=%d\n",
		ledger.Counts.Total,
		ledger.Counts.Unreviewed,
		ledger.Counts.Refactored,
		ledger.Counts.Retained,
		ledger.Counts.Deferred,
		ledger.Counts.Stale,
	)
	for index := range ledger.Items {
		writeTextReviewItem(out, &ledger.Items[index])
	}

	return nil
}

func writeTextReviewItem(out *strings.Builder, item *report.ReviewItem) {
	fmt.Fprintf(out, "- %s %s [%s]", item.Kind, item.ID, item.Status)
	if item.Stale {
		out.WriteString(" stale=true")
	}
	if len(item.Locations) > 0 {
		out.WriteString(" locations=")
		for locationIndex := range item.Locations {
			if locationIndex > 0 {
				out.WriteString(";")
			}

			location := item.Locations[locationIndex]
			out.WriteString(formatReportLocation(location.Path, location.Start, location.End))
		}
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
	out.WriteByte('\n')
}

func writeMetricBucket(out *strings.Builder, name string, bucket report.MetricBucket) {
	fmt.Fprintf(
		out,
		"%s metrics: code_lines=%d functions=%d cyclomatic=%d max_nesting=%d mass=%.6f high_complexity_functions=%d high_complexity_mass=%.6f",
		name,
		bucket.CodeLines,
		bucket.Functions,
		bucket.Cyclomatic,
		bucket.MaxNesting,
		bucket.Mass,
		bucket.HighComplexityFunctions,
		bucket.HighComplexityMass,
	)
	if bucket.HighComplexityMassShare == nil {
		out.WriteString(" high_complexity_mass_share=null")
	} else {
		fmt.Fprintf(out, " high_complexity_mass_share=%.6f", *bucket.HighComplexityMassShare)
	}

	out.WriteByte('\n')
}

func writeFunctionHotspots(out *strings.Builder, bucket string, functions []report.Function) {
	shown := rankFunctions(functions)
	fmt.Fprintf(out, "%s hotspots (%d of %d)\n", bucket, len(shown), len(functions))
	for index := range shown {
		function := shown[index]
		fmt.Fprintf(
			out,
			"- %s:%d:%d-%d:%d %s [%s] sloc=%d cyclomatic=%d nesting=%d mass=%.6f",
			function.Path,
			function.Start.Line,
			function.Start.Column,
			function.End.Line,
			function.End.Column,
			function.Name,
			function.Category,
			function.SLOC,
			function.Cyclomatic,
			function.MaxNesting,
			function.Mass,
		)
		if function.HighComplexity {
			out.WriteString(" high_complexity=true")
		}
		fmt.Fprintf(
			out,
			" flat_guards=%d field_mappings=%d nested_branches=%d assertion_like_calls=%d identity_ambiguous=%t",
			function.StructuralSignals.FlatGuards,
			function.StructuralSignals.FieldMappings,
			function.StructuralSignals.NestedBranches,
			function.StructuralSignals.AssertionLikeCalls,
			function.IdentityAmbiguous,
		)

		out.WriteByte('\n')
	}

	if len(functions) > len(shown) {
		fmt.Fprintf(out, "  ... %d more; use JSON/HTML for the full inventory\n", len(functions)-len(shown))
	}
}

func writeTextProvenance(out *strings.Builder, value report.Provenance, label string) {
	fmt.Fprintf(
		out,
		"%s: build_revision=%s build_known=%t build_modified=%t source_commit=%s source_git_known=%t source_dirty=%t source_manifest_digest=%s\n",
		label,
		provenanceValue(value.Build.Revision),
		value.Build.Known,
		value.Build.Modified,
		provenanceValue(value.Source.Commit),
		value.Source.GitKnown,
		value.Source.Dirty,
		provenanceValue(value.Source.ManifestDigest),
	)
}

func provenanceValue(value string) string {
	if value == "" {
		return "unknown"
	}
	return value
}

func rankFunctions(functions []report.Function) []report.Function {
	ranked := rankAllFunctions(functions)
	if len(ranked) > hotspotLimit {
		ranked = ranked[:hotspotLimit]
	}

	return ranked
}

func rankAllFunctions(functions []report.Function) []report.Function {
	ranked := append([]report.Function(nil), functions...)
	sort.SliceStable(ranked, func(left, right int) bool {
		if ranked[left].Mass != ranked[right].Mass {
			return ranked[left].Mass > ranked[right].Mass
		}

		if ranked[left].MaxNesting != ranked[right].MaxNesting {
			return ranked[left].MaxNesting > ranked[right].MaxNesting
		}

		if ranked[left].Cyclomatic != ranked[right].Cyclomatic {
			return ranked[left].Cyclomatic > ranked[right].Cyclomatic
		}

		if ranked[left].Path != ranked[right].Path {
			return ranked[left].Path < ranked[right].Path
		}

		if ranked[left].Start.Line != ranked[right].Start.Line {
			return ranked[left].Start.Line < ranked[right].Start.Line
		}

		return ranked[left].ID < ranked[right].ID
	})
	return ranked
}
