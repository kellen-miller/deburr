// Package compare compares reports produced with the same analyzer contract.
package compare

import (
	"encoding/json"
	"errors"
	"fmt"
	"reflect"
	"sort"

	"github.com/kellen-miller/deburr/internal/report"
)

const SchemaVersion = "2"

type Result struct {
	MetricsDelta       *MetricsDelta           `json:"metrics_delta"`
	AfterProvenance    report.Provenance       `json:"after_provenance"`
	BeforeProvenance   report.Provenance       `json:"before_provenance"`
	BeforeAnalyzer     report.AnalyzerIdentity `json:"before_analyzer"`
	AfterAnalyzer      report.AnalyzerIdentity `json:"after_analyzer"`
	SchemaVersion      string                  `json:"schema_version"`
	Reason             string                  `json:"reason,omitempty"`
	AmbiguousClones    []AmbiguousMatch        `json:"ambiguous_clones,omitempty"`
	FunctionChanges    []FunctionComparison    `json:"function_changes"`
	ResolvedFindings   []report.Finding        `json:"resolved_findings"`
	AmbiguousFunctions []AmbiguousMatch        `json:"ambiguous_functions,omitempty"`
	CloneChanges       []CloneComparison       `json:"clone_changes"`
	Ambiguous          []AmbiguousMatch        `json:"ambiguous_findings,omitempty"`
	NewFindings        []report.Finding        `json:"new_findings"`
	FindingChanges     []FindingComparison     `json:"finding_changes"`
	BeforeScope        report.Scope            `json:"before_scope"`
	AfterScope         report.Scope            `json:"after_scope"`
	Duplication        DuplicationComparison   `json:"duplication"`
	BeforeMetrics      report.Metrics          `json:"before_metrics"`
	AfterMetrics       report.Metrics          `json:"after_metrics"`
	BeforeConfig       report.ConfigSummary    `json:"before_config"`
	AfterConfig        report.ConfigSummary    `json:"after_config"`
	BeforeReview       report.ReviewLedger     `json:"before_review"`
	AfterReview        report.ReviewLedger     `json:"after_review"`
	BeforeCoverage     report.Coverage         `json:"before_coverage"`
	AfterCoverage      report.Coverage         `json:"after_coverage"`
	Complete           bool                    `json:"complete"`
	Compatible         bool                    `json:"compatible"`
}

type ComparisonState string

const (
	ComparisonMatched           ComparisonState = "matched"
	ComparisonNew               ComparisonState = "new"
	ComparisonRemoved           ComparisonState = "removed"
	ComparisonAmbiguous         ComparisonState = "ambiguous"
	ComparisonNoLongerReported  ComparisonState = "no_longer_reported"
	ComparisonSourceRemoved     ComparisonState = "source_removed"
	ComparisonDeletedFunction   ComparisonState = "deleted_function"
	ComparisonResolvedThreshold ComparisonState = "resolved_threshold"
	ComparisonCrossedThreshold  ComparisonState = "crossed_threshold"
	ComparisonIncompatible      ComparisonState = "incompatible"
	ComparisonIncomplete        ComparisonState = "incomplete"
)

type MatchBasis string

const (
	MatchByID          MatchBasis = "id"
	MatchByFingerprint MatchBasis = "fingerprint"
)

type FindingComparison struct {
	State        ComparisonState    `json:"state"`
	MatchBasis   MatchBasis         `json:"match_basis,omitempty"`
	Before       *report.Finding    `json:"before,omitempty"`
	After        *report.Finding    `json:"after,omitempty"`
	BeforeReview *report.ReviewItem `json:"before_review,omitempty"`
	AfterReview  *report.ReviewItem `json:"after_review,omitempty"`
	BeforeIDs    []string           `json:"before_ids,omitempty"`
	AfterIDs     []string           `json:"after_ids,omitempty"`
}

type FunctionComparison struct {
	State        ComparisonState    `json:"state"`
	MatchBasis   MatchBasis         `json:"match_basis,omitempty"`
	Before       *report.Function   `json:"before,omitempty"`
	After        *report.Function   `json:"after,omitempty"`
	BeforeReview *report.ReviewItem `json:"before_review,omitempty"`
	AfterReview  *report.ReviewItem `json:"after_review,omitempty"`
	BeforeIDs    []string           `json:"before_ids,omitempty"`
	AfterIDs     []string           `json:"after_ids,omitempty"`
}

type CloneComparison struct {
	State        ComparisonState    `json:"state"`
	MatchBasis   MatchBasis         `json:"match_basis,omitempty"`
	Before       *report.Clone      `json:"before,omitempty"`
	After        *report.Clone      `json:"after,omitempty"`
	BeforeReview *report.ReviewItem `json:"before_review,omitempty"`
	AfterReview  *report.ReviewItem `json:"after_review,omitempty"`
	BeforeIDs    []string           `json:"before_ids,omitempty"`
	AfterIDs     []string           `json:"after_ids,omitempty"`
}

type MetricsDelta struct {
	All        MetricDelta `json:"all"`
	Production MetricDelta `json:"production"`
	Test       MetricDelta `json:"test"`
}

type MetricDelta struct {
	HighComplexityMassShare *float64 `json:"high_complexity_mass_share,omitempty"`
	CodeLines               int      `json:"code_lines"`
	Functions               int      `json:"functions"`
	Cyclomatic              int      `json:"cyclomatic"`
	MaxNesting              int      `json:"max_nesting"`
	Mass                    float64  `json:"mass"`
	HighComplexityFunctions int      `json:"high_complexity_functions"`
	HighComplexityMass      float64  `json:"high_complexity_mass"`
}

type AmbiguousMatch struct {
	Kind       string          `json:"kind,omitempty"`
	State      ComparisonState `json:"state,omitempty"`
	MatchBasis MatchBasis      `json:"match_basis,omitempty"`
	Rule       string          `json:"rule"`
	Path       string          `json:"path"`
	Message    string          `json:"message"`
	BeforeIDs  []string        `json:"before_ids,omitempty"`
	AfterIDs   []string        `json:"after_ids,omitempty"`
}

type DuplicationComparison struct {
	Reason       string                   `json:"reason,omitempty"`
	BeforeStatus report.DuplicationStatus `json:"before_status"`
	AfterStatus  report.DuplicationStatus `json:"after_status"`
	BeforeClones []report.Clone           `json:"before_clones,omitempty"`
	AfterClones  []report.Clone           `json:"after_clones,omitempty"`
	BeforeConfig report.DuplicationConfig `json:"before_config"`
	AfterConfig  report.DuplicationConfig `json:"after_config"`
	Comparable   bool                     `json:"comparable"`
}

func Compare(before, after *report.Report) (Result, error) {
	if err := validateInput(before); err != nil {
		return Result{}, fmt.Errorf("before report: %w", err)
	}

	if err := validateInput(after); err != nil {
		return Result{}, fmt.Errorf("after report: %w", err)
	}

	beforeReview, err := reviewLedgerForCompare(before)
	if err != nil {
		return Result{}, fmt.Errorf("before review: %w", err)
	}

	afterReview, err := reviewLedgerForCompare(after)
	if err != nil {
		return Result{}, fmt.Errorf("after review: %w", err)
	}

	result := Result{
		SchemaVersion:    SchemaVersion,
		Compatible:       true,
		Complete:         true,
		BeforeAnalyzer:   before.Analyzer,
		AfterAnalyzer:    after.Analyzer,
		BeforeConfig:     before.Config,
		AfterConfig:      after.Config,
		BeforeScope:      before.Scope,
		AfterScope:       after.Scope,
		BeforeCoverage:   before.Coverage,
		AfterCoverage:    after.Coverage,
		BeforeMetrics:    before.Metrics,
		AfterMetrics:     after.Metrics,
		BeforeProvenance: before.Provenance,
		AfterProvenance:  after.Provenance,
		BeforeReview:     beforeReview,
		AfterReview:      afterReview,
		Duplication:      compareDuplication(before.Duplication, after.Duplication),
	}

	if reason := compatibilityReason(before, after); reason != "" {
		result.Compatible = false
		result.Complete = false
		result.Reason = reason
		return result, nil
	}

	result.MetricsDelta = &MetricsDelta{
		All:        metricDelta(before.Metrics.All, after.Metrics.All),
		Production: metricDelta(before.Metrics.Production, after.Metrics.Production),
		Test:       metricDelta(before.Metrics.Test, after.Metrics.Test),
	}

	result.FindingChanges, result.Ambiguous = compareFindings(
		before,
		after,
		&beforeReview,
		&afterReview,
	)
	result.FunctionChanges, result.AmbiguousFunctions = compareFunctions(
		before,
		after,
		&beforeReview,
		&afterReview,
	)
	result.CloneChanges, result.AmbiguousClones = compareClones(
		before,
		after,
		&beforeReview,
		&afterReview,
	)
	result.NewFindings, result.ResolvedFindings = findingAggregates(result.FindingChanges)
	return result, nil
}

func validateInput(value *report.Report) error {
	if value == nil {
		return errors.New("report is required")
	}

	if value.SchemaVersion == "" {
		return errors.New("schema_version is required")
	}

	if value.SchemaVersion != report.SchemaVersion {
		return fmt.Errorf("unsupported schema_version %q", value.SchemaVersion)
	}

	if value.Analyzer.ID == "" || value.Analyzer.Version == "" {
		return errors.New("analyzer id and version are required")
	}

	if value.Config.Language == "" {
		return errors.New("config language is required")
	}

	if len(value.Scope.Languages) == 0 {
		return errors.New("scope languages are required")
	}

	return nil
}

func compatibilityReason(before, after *report.Report) string {
	if before.SchemaVersion != after.SchemaVersion {
		return fmt.Sprintf("schema versions differ: %s vs %s", before.SchemaVersion, after.SchemaVersion)
	}

	if before.Analyzer != after.Analyzer {
		return "analyzer identity differs"
	}

	if reason := duplicationCompatibilityReason(before, after); reason != "" {
		return reason
	}

	if !sameJSON(before.Config, after.Config) {
		return "configuration differs"
	}

	if !sameJSON(before.Scope, after.Scope) {
		return "analysis scope differs"
	}

	if coverageIncomplete(&before.Coverage) || coverageIncomplete(&after.Coverage) {
		return "coverage is incomplete; findings and improvement cannot be compared"
	}

	return ""
}

func duplicationCompatibilityReason(before, after *report.Report) string {
	beforeConfig := before.Config.Duplication
	afterConfig := after.Config.Duplication
	if before.Duplication != nil {
		beforeConfig = before.Duplication.Config
	}
	if after.Duplication != nil {
		afterConfig = after.Duplication.Config
	}
	if !beforeConfig.Requested && !afterConfig.Requested {
		return ""
	}

	if beforeConfig.Requested != afterConfig.Requested {
		return "duplication request differs"
	}

	if beforeConfig.Tool != afterConfig.Tool || beforeConfig.Version != afterConfig.Version {
		return "duplication tool or version differs"
	}

	beforeStatus := duplicationStatus(before.Duplication)
	afterStatus := duplicationStatus(after.Duplication)
	if beforeStatus != report.DuplicationMeasured || afterStatus != report.DuplicationMeasured {
		return "requested duplication measurement is incomplete"
	}

	return ""
}

func coverageIncomplete(value *report.Coverage) bool {
	if value == nil {
		return true
	}

	if value.ReadErrors > 0 || value.ParseErrors > 0 {
		return true
	}

	return value.Analyzed == 0
}

func sameJSON(left, right any) bool {
	leftData, leftErr := json.Marshal(left)
	rightData, rightErr := json.Marshal(right)
	return leftErr == nil && rightErr == nil && reflect.DeepEqual(leftData, rightData)
}

func metricDelta(before, after report.MetricBucket) MetricDelta {
	var share *float64
	if before.HighComplexityMassShare != nil && after.HighComplexityMassShare != nil {
		value := *after.HighComplexityMassShare - *before.HighComplexityMassShare
		share = &value
	}

	return MetricDelta{
		CodeLines:               after.CodeLines - before.CodeLines,
		Functions:               after.Functions - before.Functions,
		Cyclomatic:              after.Cyclomatic - before.Cyclomatic,
		MaxNesting:              after.MaxNesting - before.MaxNesting,
		Mass:                    after.Mass - before.Mass,
		HighComplexityFunctions: after.HighComplexityFunctions - before.HighComplexityFunctions,
		HighComplexityMass:      after.HighComplexityMass - before.HighComplexityMass,
		HighComplexityMassShare: share,
	}
}

func sortFindings(findings []report.Finding) {
	sort.Slice(findings, func(left, right int) bool {
		if findings[left].Path != findings[right].Path {
			return findings[left].Path < findings[right].Path
		}

		if findings[left].Start.Line != findings[right].Start.Line {
			return findings[left].Start.Line < findings[right].Start.Line
		}

		if findings[left].Start.Column != findings[right].Start.Column {
			return findings[left].Start.Column < findings[right].Start.Column
		}

		if findings[left].Rule != findings[right].Rule {
			return findings[left].Rule < findings[right].Rule
		}

		return findings[left].ID < findings[right].ID
	})
}

func findingIDs(findings []report.Finding, indexes []int) []string {
	ids := make([]string, 0, len(indexes))
	for _, index := range indexes {
		ids = append(ids, findings[index].ID)
	}

	sort.Strings(ids)
	return ids
}

func compareDuplication(before, after *report.Duplication) DuplicationComparison {
	result := DuplicationComparison{
		Comparable:   true,
		BeforeStatus: duplicationStatus(before),
		AfterStatus:  duplicationStatus(after),
	}

	if before != nil {
		result.BeforeConfig = before.Config
		result.BeforeClones = append([]report.Clone(nil), before.Clones...)
	}

	if after != nil {
		result.AfterConfig = after.Config
		result.AfterClones = append([]report.Clone(nil), after.Clones...)
	}

	if result.BeforeStatus != report.DuplicationMeasured || result.AfterStatus != report.DuplicationMeasured {
		result.Comparable = false
		result.Reason = "duplication is comparable only when both reports measured it"
		return result
	}

	if !sameJSON(result.BeforeConfig, result.AfterConfig) {
		result.Comparable = false
		result.Reason = "duplication configuration differs"
	}

	return result
}

func duplicationStatus(value *report.Duplication) report.DuplicationStatus {
	if value == nil {
		return report.DuplicationNotRequested
	}

	return value.Status
}
