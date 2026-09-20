package render

import (
	"fmt"
	"io"
	"slices"
	"strings"

	"github.com/kellen-miller/deburr/internal/compare"
	"github.com/kellen-miller/deburr/internal/report"
)

const githubWarningLevel = "warning"

func writeGitHub(w io.Writer, value *report.Report) error {
	if err := Validate(value); err != nil {
		return err
	}

	var out strings.Builder
	writeGitHubProvenance(&out, value.Provenance, "deburr provenance")
	writeGitHubCoverage(&out, value)
	writeGitHubFileFailures(&out, value)
	writeGitHubFunctions(&out, value)
	writeGitHubFindings(&out, value)
	writeGitHubClones(&out, value)
	if err := writeGitHubReview(&out, value); err != nil {
		return err
	}

	if _, err := io.WriteString(w, out.String()); err != nil {
		return fmt.Errorf("write GitHub report: %w", err)
	}

	return nil
}

func writeGitHubReview(out *strings.Builder, value *report.Report) error {
	prepared, err := reviewReportForRender(value)
	if err != nil {
		return fmt.Errorf("prepare review output: %w", err)
	}

	ledger := prepared.Review
	fmt.Fprintf(
		out,
		"::notice title=deburr review::total=%d,unreviewed=%d,refactored=%d,retained=%d,deferred=%d,stale=%d\n",
		ledger.Counts.Total,
		ledger.Counts.Unreviewed,
		ledger.Counts.Refactored,
		ledger.Counts.Retained,
		ledger.Counts.Deferred,
		ledger.Counts.Stale,
	)
	for index := range ledger.Items {
		writeGitHubReviewItem(out, &ledger.Items[index])
	}

	return nil
}

func writeGitHubReviewItem(out *strings.Builder, item *report.ReviewItem) {
	if item.Status == report.ReviewUnreviewed && !item.Stale {
		return
	}

	properties := "title=deburr review"
	if len(item.Locations) > 0 {
		location := &item.Locations[0]
		properties = "file=" + escapeCommandProperty(location.Path)
		if location.Start.Line > 0 {
			properties += fmt.Sprintf(",line=%d", location.Start.Line)
		}
		if location.End.Line > 0 {
			properties += fmt.Sprintf(",endLine=%d", location.End.Line)
		}
		properties += ",title=deburr review"
	}

	message := fmt.Sprintf("%s %s status=%s", item.Kind, item.ID, item.Status)
	if item.Stale {
		message += " stale=true"
	}
	if item.ReasonCategory != "" {
		message += " reason_category=" + item.ReasonCategory
	}
	if item.Reason != "" {
		message += " reason=" + item.Reason
	}
	if item.Evidence != "" {
		message += " evidence=" + item.Evidence
	}
	if item.Upstream != "" {
		message += " upstream=" + item.Upstream
	}
	if item.Previous != nil {
		message += " previous=" + formatAuditHTMLPrevious(item.Previous)
	}
	fmt.Fprintf(out, "::notice %s::%s\n", properties, escapeCommandValue(message))
}

func writeGitHubCoverage(out *strings.Builder, value *report.Report) {
	fmt.Fprintf(
		out,
		"::notice title=deburr coverage::discovered=%d,analyzed=%d,excluded=%d,unsupported=%d,read_errors=%d,parse_errors=%d,symlinks=%d,excluded_directories=%d\n",
		value.Coverage.Discovered,
		value.Coverage.Analyzed,
		value.Coverage.Excluded,
		value.Coverage.Unsupported,
		value.Coverage.ReadErrors,
		value.Coverage.ParseErrors,
		value.Coverage.Symlinks,
		value.Coverage.ExcludedDirectories,
	)
	if value.Coverage.ReadErrors > 0 || value.Coverage.ParseErrors > 0 {
		fmt.Fprintf(
			out,
			"::error title=deburr analysis::analysis incomplete: read_errors=%d,parse_errors=%d\n",
			value.Coverage.ReadErrors,
			value.Coverage.ParseErrors,
		)
	}

	if value.Coverage.Analyzed == 0 {
		fmt.Fprint(out, "::notice title=deburr analysis::no analyzable Go files\n")
	}

	if value.Duplication != nil && value.Duplication.Status == report.DuplicationError {
		fmt.Fprintf(out, "::error title=deburr duplication::%s\n", escapeCommandValue(value.Duplication.Error))
	}
}

func writeGitHubFileFailures(out *strings.Builder, value *report.Report) {
	for index := range value.Files {
		file := &value.Files[index]
		if file.Status != report.FileReadError && file.Status != report.FileParseError {
			continue
		}

		message := file.Error
		if message == "" {
			message = file.Reason
		}

		properties := "file=" + escapeCommandProperty(file.Path) + ",title=deburr analysis"
		fmt.Fprintf(out, "::error %s::%s\n", properties, escapeCommandValue(message))
	}
}

func writeGitHubFindings(out *strings.Builder, value *report.Report) {
	for index := range value.Findings {
		finding := &value.Findings[index]
		writeGitHubFinding(out, githubFindingLevel(finding.Severity), finding, "")
	}
}

func githubFindingLevel(severity string) string {
	level := strings.ToLower(severity)
	switch level {
	case "error", githubWarningLevel, "notice":
		return level
	default:
		return "warning"
	}
}

func writeGitHubClones(out *strings.Builder, value *report.Report) {
	if value.Duplication == nil || value.Duplication.Status != report.DuplicationMeasured {
		return
	}

	for cloneIndex := range value.Duplication.Clones {
		clone := &value.Duplication.Clones[cloneIndex]
		for locationIndex := range clone.Locations {
			location := &clone.Locations[locationIndex]
			properties := "file=" + escapeCommandProperty(location.Path)
			if location.Start.Line > 0 {
				properties += fmt.Sprintf(",line=%d", location.Start.Line)
			}

			if location.End.Line > 0 {
				properties += fmt.Sprintf(",endLine=%d", location.End.Line)
			}

			fmt.Fprintf(
				out,
				"::warning %s,title=deburr duplication::clone %s family=%s identity_ambiguous=%t\n",
				properties,
				escapeCommandValue(clone.ID),
				escapeCommandValue(clone.FamilyID),
				clone.IdentityAmbiguous,
			)
		}
	}
}

func writeComparisonGitHub(w io.Writer, value *compare.Result) error {
	if err := ValidateComparison(value); err != nil {
		return err
	}

	var out strings.Builder
	writeGitHubProvenance(&out, value.BeforeProvenance, "deburr comparison before provenance")
	writeGitHubProvenance(&out, value.AfterProvenance, "deburr comparison after provenance")
	fmt.Fprintf(
		&out,
		"::notice title=deburr comparison::compatible=%t,complete=%t,before_analyzed=%d,after_analyzed=%d\n",
		value.Compatible,
		value.Complete,
		value.BeforeCoverage.Analyzed,
		value.AfterCoverage.Analyzed,
	)
	if !value.Compatible || !value.Complete {
		message := value.Reason
		if message == "" {
			message = "comparison unavailable"
		}

		fmt.Fprintf(&out, "::error title=deburr comparison::%s\n", escapeCommandValue(message))
	}

	fmt.Fprintf(
		&out,
		"::notice title=deburr comparison review::before_total=%d,before_unreviewed=%d,before_refactored=%d,before_retained=%d,after_total=%d,after_unreviewed=%d,after_refactored=%d,after_retained=%d\n",
		value.BeforeReview.Counts.Total,
		value.BeforeReview.Counts.Unreviewed,
		value.BeforeReview.Counts.Refactored,
		value.BeforeReview.Counts.Retained,
		value.AfterReview.Counts.Total,
		value.AfterReview.Counts.Unreviewed,
		value.AfterReview.Counts.Refactored,
		value.AfterReview.Counts.Retained,
	)
	writeGitHubFindingChanges(&out, value)
	writeGitHubFunctionChanges(&out, value)
	writeGitHubCloneChanges(&out, value)

	if len(value.Ambiguous) > 0 {
		fmt.Fprintf(
			&out,
			"::notice title=deburr comparison::%d ambiguous finding matches; no new or resolved claim made\n",
			len(value.Ambiguous),
		)
	}

	if !value.Duplication.Comparable {
		fmt.Fprintf(
			&out,
			"::notice title=deburr duplication::before=%s,after=%s,%s\n",
			value.Duplication.BeforeStatus,
			value.Duplication.AfterStatus,
			escapeCommandValue(value.Duplication.Reason),
		)
	}

	if _, err := io.WriteString(w, out.String()); err != nil {
		return fmt.Errorf("write comparison GitHub report: %w", err)
	}

	return nil
}

func writeGitHubProvenance(out *strings.Builder, value report.Provenance, title string) {
	fmt.Fprintf(
		out,
		"::notice title=%s::build_revision=%s,build_known=%t,build_modified=%t,source_commit=%s,source_git_known=%t,source_dirty=%t,source_manifest_digest=%s\n",
		escapeCommandProperty(title),
		escapeCommandValue(provenanceValue(value.Build.Revision)),
		value.Build.Known,
		value.Build.Modified,
		escapeCommandValue(provenanceValue(value.Source.Commit)),
		value.Source.GitKnown,
		value.Source.Dirty,
		escapeCommandValue(provenanceValue(value.Source.ManifestDigest)),
	)
}

func writeGitHubFunctions(out *strings.Builder, value *report.Report) {
	for index := range value.Functions {
		function := &value.Functions[index]
		properties := "file=" + escapeCommandProperty(function.Path)
		if function.Start.Line > 0 {
			properties += fmt.Sprintf(",line=%d", function.Start.Line)
		}
		if function.End.Line > 0 {
			properties += fmt.Sprintf(",endLine=%d", function.End.Line)
		}
		message := fmt.Sprintf(
			"%s sloc=%d cyclomatic=%d nesting=%d mass=%.6f flat_guards=%d field_mappings=%d nested_branches=%d assertion_like_calls=%d identity_ambiguous=%t",
			function.Name,
			function.SLOC,
			function.Cyclomatic,
			function.MaxNesting,
			function.Mass,
			function.StructuralSignals.FlatGuards,
			function.StructuralSignals.FieldMappings,
			function.StructuralSignals.NestedBranches,
			function.StructuralSignals.AssertionLikeCalls,
			function.IdentityAmbiguous,
		)
		fmt.Fprintf(out, "::notice %s,title=deburr function::%s\n", properties, escapeCommandValue(message))
	}
}

func writeGitHubFindingChanges(out *strings.Builder, value *compare.Result) {
	for index := range value.FindingChanges {
		change := &value.FindingChanges[index]
		switch change.State {
		case compare.ComparisonNew:
			if change.After != nil {
				writeGitHubFinding(out, "warning", change.After, "new: ")
			}
		case compare.ComparisonNoLongerReported, compare.ComparisonSourceRemoved:
			if change.Before != nil {
				writeGitHubFinding(out, "notice", change.Before, string(change.State)+": ")
			}
		case compare.ComparisonAmbiguous:
			writeGitHubAmbiguous(out, change.State, report.ReviewFinding, change.BeforeIDs, change.AfterIDs)
		default:
			continue
		}
	}
}

func writeGitHubFunctionChanges(out *strings.Builder, value *compare.Result) {
	for index := range value.FunctionChanges {
		change := &value.FunctionChanges[index]
		function := change.After
		if function == nil {
			function = change.Before
		}
		if function == nil {
			continue
		}
		properties := "file=" + escapeCommandProperty(function.Path)
		if function.Start.Line > 0 {
			properties += fmt.Sprintf(",line=%d", function.Start.Line)
		}
		if function.End.Line > 0 {
			properties += fmt.Sprintf(",endLine=%d", function.End.Line)
		}
		level := "notice"
		if change.State == compare.ComparisonNew || change.State == compare.ComparisonCrossedThreshold {
			level = githubWarningLevel
		}
		message := fmt.Sprintf("%s state=%s; no behavior conclusion", function.Name, change.State)
		fmt.Fprintf(
			out,
			"::%s %s,title=deburr function comparison::%s\n",
			level,
			properties,
			escapeCommandValue(message),
		)
		if change.State == compare.ComparisonAmbiguous {
			writeGitHubAmbiguous(out, change.State, report.ReviewFunction, change.BeforeIDs, change.AfterIDs)
		}
	}
}

func writeGitHubCloneChanges(out *strings.Builder, value *compare.Result) {
	for index := range value.CloneChanges {
		change := &value.CloneChanges[index]
		clone := change.After
		if clone == nil {
			clone = change.Before
		}
		if clone == nil {
			continue
		}
		message := fmt.Sprintf(
			"clone %s family=%s state=%s identity_ambiguous=%t",
			clone.ID,
			clone.FamilyID,
			change.State,
			clone.IdentityAmbiguous,
		)
		if len(clone.Locations) == 0 {
			fmt.Fprintf(out, "::notice title=deburr clone comparison::%s\n", escapeCommandValue(message))
		}
		for locationIndex := range clone.Locations {
			location := &clone.Locations[locationIndex]
			properties := "file=" + escapeCommandProperty(location.Path)
			if location.Start.Line > 0 {
				properties += fmt.Sprintf(",line=%d", location.Start.Line)
			}
			if location.End.Line > 0 {
				properties += fmt.Sprintf(",endLine=%d", location.End.Line)
			}
			fmt.Fprintf(out, "::notice %s,title=deburr clone comparison::%s\n", properties, escapeCommandValue(message))
		}
		if change.State == compare.ComparisonAmbiguous {
			writeGitHubAmbiguous(out, change.State, report.ReviewClone, change.BeforeIDs, change.AfterIDs)
		}
	}
}

func writeGitHubAmbiguous(
	out *strings.Builder,
	state compare.ComparisonState,
	kind report.ReviewItemKind,
	before, after []string,
) {
	fmt.Fprintf(
		out,
		"::notice title=deburr comparison::%s %s before=%s after=%s; no change claim made\n",
		kind,
		state,
		escapeCommandValue(strings.Join(before, ",")),
		escapeCommandValue(strings.Join(after, ",")),
	)
}

func writeGitHubFinding(out *strings.Builder, level string, finding *report.Finding, prefix string) {
	properties := "file=" + escapeCommandProperty(finding.Path)
	if finding.Start.Line > 0 {
		properties += fmt.Sprintf(",line=%d", finding.Start.Line)
	}

	if finding.End.Line > 0 {
		properties += fmt.Sprintf(",endLine=%d", finding.End.Line)
	}

	if finding.Rule != "" {
		properties += ",title=" + escapeCommandProperty(finding.Rule)
	}

	fmt.Fprintf(
		out,
		"::%s %s::%s%s identity_ambiguous=%t",
		level,
		properties,
		escapeCommandValue(prefix),
		escapeCommandValue(finding.Message),
		finding.IdentityAmbiguous,
	)
	out.WriteByte('\n')
}

func escapeCommandProperty(value string) string {
	return escapeCommandValue(value, ':', ',')
}

func escapeCommandValue(value string, extra ...rune) string {
	var out strings.Builder
	for _, char := range value {
		if escaped, ok := escapeCommandCharacter(char, extra); ok {
			out.WriteString(escaped)
			continue
		}

		out.WriteRune(char)
	}

	return out.String()
}

func escapeCommandCharacter(char rune, extra []rune) (string, bool) {
	switch char {
	case '%':
		return "%25", true
	case '\r':
		return "%0D", true
	case '\n':
		return "%0A", true
	case ':':
		if slices.Contains(extra, char) {
			return "%3A", true
		}
	case ',':
		if slices.Contains(extra, char) {
			return "%2C", true
		}
	}

	return "", false
}
