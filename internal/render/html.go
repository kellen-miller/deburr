package render

import (
	"fmt"
	"html/template"
	"io"
	"sort"
	"strings"

	"github.com/kellen-miller/deburr/internal/compare"
	"github.com/kellen-miller/deburr/internal/report"
)

type auditHTMLView struct {
	InventoryRows   []auditHTMLInventoryRow
	CategoryOptions []string
	StatusOptions   []string
	report.Report
	ReviewSummary report.ReviewCounts
	FunctionCount int
	HotspotCount  int
	FindingCount  int
	CloneCount    int
	ReviewCount   int
}

type auditHTMLInventoryRow struct {
	Kind           string
	Category       string
	Status         string
	ID             string
	Fingerprint    string
	Subject        string
	Location       string
	Metrics        string
	ReasonCategory string
	Reason         string
	Evidence       string
	Upstream       string
	Previous       string
	Search         string
	Review         bool
	Stale          bool
}

const auditHTMLObservedStatus = "observed"

func newAuditHTMLView(value *report.Report) (auditHTMLView, error) {
	viewValue, err := reviewReportForRender(value)
	if err != nil {
		return auditHTMLView{}, err
	}
	ledger := viewValue.Review

	functions := rankAllFunctions(viewValue.Functions)
	findings := append([]report.Finding(nil), viewValue.Findings...)
	sort.SliceStable(findings, func(left, right int) bool {
		return findingSortKey(&findings[left]) < findingSortKey(&findings[right])
	})
	clones := make([]report.Clone, 0)
	if viewValue.Duplication != nil {
		clones = append(clones, viewValue.Duplication.Clones...)
	}
	sort.SliceStable(clones, func(left, right int) bool {
		return cloneSortKey(&clones[left]) < cloneSortKey(&clones[right])
	})

	rows := makeAuditHTMLInventoryRows(&viewValue, functions, findings, clones, ledger)
	categories, statuses := auditHTMLFilterOptions(rows)
	hotspotCount := 0
	for index := range functions {
		if functions[index].HighComplexity {
			hotspotCount++
		}
	}

	return auditHTMLView{
		Report:          viewValue,
		ReviewSummary:   ledger.Counts,
		InventoryRows:   rows,
		CategoryOptions: categories,
		StatusOptions:   statuses,
		FunctionCount:   len(functions),
		HotspotCount:    hotspotCount,
		FindingCount:    len(findings),
		CloneCount:      len(clones),
		ReviewCount:     ledger.Counts.Total,
	}, nil
}

type auditHTMLReviewIndex struct {
	items map[string]report.ReviewItem
}

func newAuditHTMLReviewIndex(ledger *report.ReviewLedger) auditHTMLReviewIndex {
	index := auditHTMLReviewIndex{
		items: make(map[string]report.ReviewItem),
	}
	if ledger == nil {
		return index
	}

	for itemIndex := range ledger.Items {
		item := ledger.Items[itemIndex]
		index.items[item.ID] = item
	}

	return index
}

func (index *auditHTMLReviewIndex) take(
	id string,
) (report.ReviewItem, bool) {
	item, ok := index.items[id]
	if !ok {
		return report.ReviewItem{}, false
	}

	return item, true
}

func makeAuditHTMLInventoryRows(
	value *report.Report,
	functions []report.Function,
	findings []report.Finding,
	clones []report.Clone,
	ledger *report.ReviewLedger,
) []auditHTMLInventoryRow {
	categoryByPath := make(map[string]string, len(value.Files))
	for index := range value.Files {
		file := value.Files[index]
		categoryByPath[file.Path] = string(file.Category)
	}

	reviewIndex := newAuditHTMLReviewIndex(ledger)
	rows := make([]auditHTMLInventoryRow, 0, len(functions)+len(findings)+len(clones))
	rows = appendAuditHTMLFunctionRows(rows, functions, &reviewIndex)
	rows = appendAuditHTMLFindingRows(rows, findings, categoryByPath, &reviewIndex)
	rows = appendAuditHTMLCloneRows(rows, clones, &reviewIndex)

	for index := range rows {
		row := &rows[index]
		row.Search = strings.Join(
			[]string{
				row.Kind,
				row.Category,
				row.Status,
				row.ID,
				row.Fingerprint,
				row.Subject,
				row.Location,
				row.Metrics,
				row.ReasonCategory,
				row.Reason,
				row.Evidence,
				row.Upstream,
				row.Previous,
			},
			" ",
		)
	}

	return rows
}

func appendAuditHTMLFunctionRows(
	rows []auditHTMLInventoryRow,
	functions []report.Function,
	reviewIndex *auditHTMLReviewIndex,
) []auditHTMLInventoryRow {
	for index := range functions {
		function := functions[index]
		reason := ""
		if function.HighComplexity {
			reason = "high_complexity=true"
		}

		row := auditHTMLInventoryRow{
			Kind:     "function",
			Category: auditHTMLCategory(function.Category),
			Status:   auditHTMLObservedStatus,
			ID:       function.ID,
			Subject:  function.Name,
			Location: formatReportLocation(function.Path, function.Start, function.End),
			Metrics: fmt.Sprintf(
				"sloc=%d cyclomatic=%d max_nesting=%d mass=%.6f flat_guards=%d field_mappings=%d nested_branches=%d assertion_like_calls=%d identity_ambiguous=%t",
				function.SLOC,
				function.Cyclomatic,
				function.MaxNesting,
				function.Mass,
				function.StructuralSignals.FlatGuards,
				function.StructuralSignals.FieldMappings,
				function.StructuralSignals.NestedBranches,
				function.StructuralSignals.AssertionLikeCalls,
				function.IdentityAmbiguous,
			),
			Reason: reason,
			Review: function.HighComplexity,
		}
		if function.HighComplexity {
			item, matched := reviewIndex.take(function.ID)
			if matched {
				applyAuditHTMLReview(&row, &item)
			}
		}
		rows = append(rows, row)
	}

	return rows
}

func appendAuditHTMLFindingRows(
	rows []auditHTMLInventoryRow,
	findings []report.Finding,
	categoryByPath map[string]string,
	reviewIndex *auditHTMLReviewIndex,
) []auditHTMLInventoryRow {
	for index := range findings {
		finding := findings[index]
		category := auditHTMLCategory(report.Category(categoryByPath[finding.Path]))

		row := auditHTMLInventoryRow{
			Kind:     "finding",
			Category: category,
			Status:   auditHTMLObservedStatus,
			ID:       finding.ID,
			Subject:  finding.Rule,
			Location: formatReportLocation(finding.Path, finding.Start, finding.End),
			Metrics: fmt.Sprintf(
				"severity=%s identity_ambiguous=%t",
				finding.Severity,
				finding.IdentityAmbiguous,
			),
			Reason: finding.Message,
			Review: true,
		}
		item, matched := reviewIndex.take(finding.ID)
		if matched {
			applyAuditHTMLReview(&row, &item)
		}
		rows = append(rows, row)
	}

	return rows
}

func appendAuditHTMLCloneRows(
	rows []auditHTMLInventoryRow,
	clones []report.Clone,
	reviewIndex *auditHTMLReviewIndex,
) []auditHTMLInventoryRow {
	for index := range clones {
		clone := clones[index]
		locations := make([]string, 0, len(clone.Locations))
		for locationIndex := range clone.Locations {
			location := clone.Locations[locationIndex]
			locations = append(
				locations,
				formatReportLocation(location.Path, location.Start, location.End),
			)
		}

		row := auditHTMLInventoryRow{
			Kind:     "clone",
			Category: auditHTMLCategory(clone.Category),
			Status:   auditHTMLObservedStatus,
			ID:       clone.ID,
			Subject:  "duplicate source spans",
			Location: strings.Join(locations, "; "),
			Metrics: fmt.Sprintf(
				"family_id=%s tokens=%d lines=%d identity_ambiguous=%t",
				clone.FamilyID,
				clone.Tokens,
				clone.Lines,
				clone.IdentityAmbiguous,
			),
			Review: true,
		}
		item, matched := reviewIndex.take(clone.ID)
		if matched {
			applyAuditHTMLReview(&row, &item)
		}
		rows = append(rows, row)
	}

	return rows
}

func applyAuditHTMLReview(row *auditHTMLInventoryRow, item *report.ReviewItem) {
	row.Status = string(item.Status)
	row.Fingerprint = item.Fingerprint
	row.Reason = item.Reason
	row.ReasonCategory = item.ReasonCategory
	row.Evidence = item.Evidence
	row.Upstream = item.Upstream
	row.Previous = formatAuditHTMLPrevious(item.Previous)
	row.Stale = item.Stale
}

func formatAuditHTMLPrevious(previous *report.ReviewDecision) string {
	if previous == nil {
		return ""
	}

	values := []string{string(previous.Status)}
	if previous.ReasonCategory != "" {
		values = append(values, "reason_category="+previous.ReasonCategory)
	}
	if previous.Reason != "" {
		values = append(values, "reason="+previous.Reason)
	}
	if previous.Evidence != "" {
		values = append(values, "evidence="+previous.Evidence)
	}
	if previous.Upstream != "" {
		values = append(values, "upstream="+previous.Upstream)
	}
	if previous.Fingerprint != "" {
		values = append(values, "fingerprint="+previous.Fingerprint)
	}

	return strings.Join(values, "; ")
}

func auditHTMLCategory(category report.Category) string {
	if category == "" {
		return "unknown"
	}

	return string(category)
}

func auditHTMLFilterOptions(rows []auditHTMLInventoryRow) ([]string, []string) {
	categorySet := make(map[string]struct{})
	statusSet := make(map[string]struct{})
	for index := range rows {
		categorySet[rows[index].Category] = struct{}{}
		statusSet[rows[index].Status] = struct{}{}
	}

	categories := make([]string, 0, len(categorySet))
	for category := range categorySet {
		categories = append(categories, category)
	}
	sort.Strings(categories)

	statuses := make([]string, 0, len(statusSet))
	for status := range statusSet {
		statuses = append(statuses, status)
	}
	sort.Strings(statuses)
	return categories, statuses
}

func findingSortKey(finding *report.Finding) string {
	return fmt.Sprintf(
		"%s:%09d:%09d:%s:%s",
		finding.Path,
		finding.Start.Line,
		finding.Start.Column,
		finding.Rule,
		finding.ID,
	)
}

func cloneSortKey(clone *report.Clone) string {
	location := ""
	if len(clone.Locations) > 0 {
		location = formatReportLocation(
			clone.Locations[0].Path,
			clone.Locations[0].Start,
			clone.Locations[0].End,
		)
	}

	return fmt.Sprintf("%s:%s:%09d:%s", clone.Category, location, clone.Tokens, clone.ID)
}

func formatReportLocation(path string, start, end report.Position) string {
	return fmt.Sprintf(
		"%s:%d:%d-%d:%d",
		path,
		start.Line,
		start.Column,
		end.Line,
		end.Column,
	)
}

func writeComparisonHTML(w io.Writer, value *compare.Result) error {
	if err := ValidateComparison(value); err != nil {
		return err
	}

	tmpl, err := template.New("comparison").Funcs(htmlTemplateFuncs()).Parse(comparisonHTMLPage)
	if err != nil {
		return fmt.Errorf("parse comparison HTML template: %w", err)
	}

	if err := tmpl.Execute(w, value); err != nil {
		return fmt.Errorf("write comparison HTML: %w", err)
	}

	return nil
}

func writeHTML(w io.Writer, value *report.Report) error {
	if err := Validate(value); err != nil {
		return err
	}

	tmpl, err := template.New("report").Funcs(htmlTemplateFuncs()).Parse(htmlPage)
	if err != nil {
		return fmt.Errorf("parse HTML template: %w", err)
	}

	view, err := newAuditHTMLView(value)
	if err != nil {
		return err
	}

	if err := tmpl.Execute(w, view); err != nil {
		return fmt.Errorf("write HTML report: %w", err)
	}

	return nil
}

func htmlTemplateFuncs() template.FuncMap {
	return template.FuncMap{
		"formatShare":            formatShare,
		"provenanceText":         provenanceText,
		"findingComparisonText":  findingComparisonText,
		"functionComparisonText": functionComparisonText,
		"cloneComparisonText":    cloneComparisonText,
		"reviewComparisonText":   reviewComparisonText,
		"comparisonIDs":          comparisonIDs,
	}
}

func formatShare(value *float64) string {
	if value == nil {
		return "null"
	}

	return fmt.Sprintf("%.6f", *value)
}

func provenanceText(value report.Provenance) string {
	return fmt.Sprintf(
		"build_revision=%s build_known=%t build_modified=%t source_commit=%s source_git_known=%t source_dirty=%t source_manifest_digest=%s",
		provenanceValue(value.Build.Revision),
		value.Build.Known,
		value.Build.Modified,
		provenanceValue(value.Source.Commit),
		value.Source.GitKnown,
		value.Source.Dirty,
		provenanceValue(value.Source.ManifestDigest),
	)
}

func findingComparisonText(value *report.Finding) string {
	if value == nil {
		return ""
	}
	return fmt.Sprintf(
		"id=%s %s %s %s ambiguous=%t",
		value.ID,
		formatReportLocation(value.Path, value.Start, value.End),
		value.Rule,
		value.Message,
		value.IdentityAmbiguous,
	)
}

func functionComparisonText(value *report.Function) string {
	if value == nil {
		return ""
	}
	return formatFunctionComparison(value)
}

func cloneComparisonText(value *report.Clone) string {
	if value == nil {
		return ""
	}
	return formatCloneComparison(value)
}

func reviewComparisonText(before, after *report.ReviewItem) string {
	values := make([]string, 0)
	for _, side := range []struct {
		item *report.ReviewItem
		name string
	}{{name: "before", item: before}, {name: "after", item: after}} {
		item := side.item
		if item == nil {
			continue
		}
		value := side.name + " status=" + string(item.Status)
		if item.Stale {
			value += " stale=true"
		}
		if item.ReasonCategory != "" {
			value += " reason_category=" + item.ReasonCategory
		}
		if item.Reason != "" {
			value += " reason=" + item.Reason
		}
		if item.Evidence != "" {
			value += " evidence=" + item.Evidence
		}
		if item.Upstream != "" {
			value += " upstream=" + item.Upstream
		}
		if item.Previous != nil {
			value += " previous=" + formatAuditHTMLPrevious(item.Previous)
		}
		values = append(values, value)
	}
	return strings.Join(values, "; ")
}

func comparisonIDs(values []string) string {
	return strings.Join(values, ", ")
}
