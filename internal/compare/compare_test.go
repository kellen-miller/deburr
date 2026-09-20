package compare

import (
	"strings"
	"testing"

	"github.com/kellen-miller/deburr/internal/report"
)

func TestCompareCompatibleReportsDeltasRawMetricsAndFindings(t *testing.T) {
	before := sampleReport()
	after := sampleReport()
	after.Metrics.All.CodeLines = 12
	after.Metrics.All.Mass = 5.5
	after.Findings[0].Start.Line = 20
	result, err := Compare(&before, &after)
	if err != nil {
		t.Fatal(err)
	}

	if !result.Compatible || !result.Complete {
		t.Fatalf("comparison = %+v", result)
	}

	if result.MetricsDelta == nil || result.MetricsDelta.All.CodeLines != 2 || result.MetricsDelta.All.Mass != 1.5 {
		t.Fatalf("metrics delta = %+v", result.MetricsDelta)
	}

	if len(result.NewFindings) != 0 || len(result.ResolvedFindings) != 0 {
		t.Fatalf(
			"line move was treated as a finding change: new=%v resolved=%v",
			result.NewFindings,
			result.ResolvedFindings,
		)
	}
}

func TestCompareIncompatibleConfigurationHasNoImprovementDelta(t *testing.T) {
	before := sampleReport()
	after := sampleReport()
	after.Config.HighComplexityThreshold = 99
	after.Metrics.All.CodeLines = 0
	after.Findings = nil
	result, err := Compare(&before, &after)
	if err != nil {
		t.Fatal(err)
	}

	if result.Compatible || result.Complete || result.MetricsDelta != nil {
		t.Fatalf("configuration change produced comparable improvement: %+v", result)
	}

	if len(result.NewFindings) != 0 || len(result.ResolvedFindings) != 0 {
		t.Fatal("incompatible reports produced finding changes")
	}
}

func TestCompareIncompleteReportsCannotClaimCleanImprovement(t *testing.T) {
	before := sampleReport()
	after := sampleReport()
	after.Coverage.ParseErrors = 1
	after.Findings = nil
	result, err := Compare(&before, &after)
	if err != nil {
		t.Fatal(err)
	}

	if result.Compatible || result.Complete || result.MetricsDelta != nil {
		t.Fatalf("partial report produced clean comparison: %+v", result)
	}
}

func TestCompareDuplicateFindingIDsAreAmbiguous(t *testing.T) {
	before := sampleReport()
	after := sampleReport()
	before.Findings = append(before.Findings, before.Findings[0])
	after.Findings = append(after.Findings, after.Findings[0])
	result, err := Compare(&before, &after)
	if err != nil {
		t.Fatal(err)
	}

	if len(result.Ambiguous) != 1 {
		t.Fatalf("ambiguous matches = %+v", result.Ambiguous)
	}

	if len(result.NewFindings) != 0 || len(result.ResolvedFindings) != 0 {
		t.Fatalf(
			"ambiguous findings were reported as changes: new=%v resolved=%v",
			result.NewFindings,
			result.ResolvedFindings,
		)
	}
}

func TestCompareIdentityAmbiguousFindingCannotMatchOrdinalID(t *testing.T) {
	before := sampleReport()
	before.Findings[0].Fingerprint = "same-source"
	before.Findings[0].IdentityAmbiguous = true
	after := before
	after.Findings = append([]report.Finding(nil), before.Findings...)
	after.Findings[0].IdentityAmbiguous = false

	result, err := Compare(&before, &after)
	if err != nil {
		t.Fatal(err)
	}
	if len(result.Ambiguous) != 1 || len(result.FindingChanges) != 1 ||
		result.FindingChanges[0].State != ComparisonAmbiguous {
		t.Fatalf(
			"ambiguous finding was matched: findings=%v changes=%v",
			result.Ambiguous,
			result.FindingChanges,
		)
	}
}

func TestCompareMatchesUniqueFingerprintAfterMove(t *testing.T) {
	before := sampleReport()
	before.Findings[0].ID = "old-path-id"
	before.Findings[0].Fingerprint = "same-source"
	after := sampleReport()
	after.Findings[0].ID = "new-path-id"
	after.Findings[0].Path = "moved.go"
	after.Findings[0].Fingerprint = "same-source"

	result, err := Compare(&before, &after)
	if err != nil {
		t.Fatal(err)
	}

	if len(result.FindingChanges) != 1 || result.FindingChanges[0].State != ComparisonMatched ||
		result.FindingChanges[0].MatchBasis != MatchByFingerprint {
		t.Fatalf("fingerprint move = %+v", result.FindingChanges)
	}
	if len(result.NewFindings) != 0 || len(result.ResolvedFindings) != 0 {
		t.Fatalf("unique move became new/resolved: new=%v resolved=%v", result.NewFindings, result.ResolvedFindings)
	}
}

func TestCompareRepeatedFingerprintWithoutStableIDsIsAmbiguous(t *testing.T) {
	before := sampleReport()
	before.Findings = []report.Finding{
		{Fingerprint: "same", Rule: "rule", Path: "main.go", Message: "one"},
		{Fingerprint: "same", Rule: "rule", Path: "main.go", Message: "two"},
	}
	after := sampleReport()
	after.Findings = []report.Finding{
		{Fingerprint: "same", Rule: "rule", Path: "main.go", Message: "one after"},
		{Fingerprint: "same", Rule: "rule", Path: "main.go", Message: "two after"},
	}

	result, err := Compare(&before, &after)
	if err != nil {
		t.Fatal(err)
	}

	if len(result.Ambiguous) != 1 || len(result.FindingChanges) != 1 ||
		result.FindingChanges[0].State != ComparisonAmbiguous {
		t.Fatalf("repeated fingerprint was inferred: ambiguous=%v changes=%v", result.Ambiguous, result.FindingChanges)
	}
	if len(result.NewFindings) != 0 || len(result.ResolvedFindings) != 0 {
		t.Fatal("ambiguous repeated regions produced new or resolved findings")
	}
}

func TestCompareFunctionThresholdTransitionAndAllFunctionInventory(t *testing.T) {
	before := sampleReport()
	before.Functions = []report.Function{
		{
			ID: "hot", Fingerprint: "hot-source", Path: "main.go", Name: "Hot",
			HighComplexity: true, Cyclomatic: 11, Mass: 12,
		},
		{ID: "small", Fingerprint: "small-source", Path: "main.go", Name: "Small", Cyclomatic: 2, Mass: 2},
	}
	after := before
	after.Functions = []report.Function{
		{
			ID: "hot", Fingerprint: "hot-source-after", Path: "main.go", Name: "Hot",
			HighComplexity: false, Cyclomatic: 3, Mass: 4,
		},
		{ID: "small", Fingerprint: "small-source", Path: "main.go", Name: "Small", Cyclomatic: 2, Mass: 2},
	}

	result, err := Compare(&before, &after)
	if err != nil {
		t.Fatal(err)
	}

	if len(result.FunctionChanges) != 2 {
		t.Fatalf("function inventory = %d, want all functions", len(result.FunctionChanges))
	}
	var hot FunctionComparison
	for index := range result.FunctionChanges {
		if result.FunctionChanges[index].Before != nil && result.FunctionChanges[index].Before.ID == "hot" {
			hot = result.FunctionChanges[index]
		}
	}
	if hot.State != ComparisonResolvedThreshold || hot.Before == nil || hot.After == nil ||
		hot.Before.Cyclomatic != 11 || hot.After.Cyclomatic != 3 {
		t.Fatalf("threshold transition = %+v", hot)
	}
}

func TestCompareSourceChangeWithEqualMetricsRemainsMatched(t *testing.T) {
	before := sampleReport()
	before.Findings[0].Fingerprint = "source-before"
	before.Provenance.Build = report.BuildIdentity{Revision: "build-before", Known: true}
	before.Provenance.Source = report.SourceIdentity{Commit: "source-before", GitKnown: true}
	after := before
	after.Findings = append([]report.Finding(nil), before.Findings...)
	after.Findings[0].Fingerprint = "source-after"
	after.Findings[0].Message = "same measurement, changed source"
	after.Provenance.Build = report.BuildIdentity{Revision: "build-after", Known: true}
	after.Provenance.Source = report.SourceIdentity{Commit: "source-after", GitKnown: true}

	result, err := Compare(&before, &after)
	if err != nil {
		t.Fatal(err)
	}
	if !result.Compatible || !result.Complete {
		t.Fatalf("provenance metadata changed compatibility: %+v", result)
	}
	if len(result.FindingChanges) != 1 || result.FindingChanges[0].State != ComparisonMatched ||
		result.FindingChanges[0].MatchBasis != MatchByID {
		t.Fatalf("source change was treated as disappearance: %+v", result.FindingChanges)
	}
}

func TestCompareIdentityAmbiguousFunctionCannotMatchOrdinalID(t *testing.T) {
	before := sampleReport()
	before.Functions = []report.Function{{
		ID: "ordinal", Fingerprint: "same", Path: "main.go", Name: "Anonymous", IdentityAmbiguous: true,
	}}
	after := before
	after.Functions = append([]report.Function(nil), before.Functions...)

	result, err := Compare(&before, &after)
	if err != nil {
		t.Fatal(err)
	}
	if len(result.AmbiguousFunctions) != 1 || len(result.FunctionChanges) != 1 ||
		result.FunctionChanges[0].State != ComparisonAmbiguous {
		t.Fatalf(
			"ambiguous function was matched: functions=%v changes=%v",
			result.AmbiguousFunctions,
			result.FunctionChanges,
		)
	}
}

func TestCompareCloneInventoryCarriesFamilyAndAmbiguity(t *testing.T) {
	before := sampleReport()
	before.Duplication.Clones = []report.Clone{{
		ID: "clone", FamilyID: "family", Fingerprint: "clone-source", IdentityAmbiguous: true,
	}}
	after := before
	after.Duplication = &report.Duplication{
		Status: report.DuplicationMeasured,
		Config: before.Duplication.Config,
		Clones: append([]report.Clone(nil), before.Duplication.Clones...),
	}

	result, err := Compare(&before, &after)
	if err != nil {
		t.Fatal(err)
	}
	if len(result.AmbiguousClones) != 1 || len(result.CloneChanges) != 1 ||
		result.CloneChanges[0].State != ComparisonAmbiguous ||
		result.CloneChanges[0].Before.FamilyID != "family" {
		t.Fatalf("clone family ambiguity was lost: clones=%v changes=%v", result.AmbiguousClones, result.CloneChanges)
	}
}

func TestCompareRemovedFunctionIsDistinctFromThresholdResolution(t *testing.T) {
	before := sampleReport()
	before.Files = []report.File{{Path: "removed.go", Status: report.FileAnalyzed}}
	before.Functions = []report.Function{{ID: "removed", Path: "removed.go", Name: "Removed", HighComplexity: true}}
	after := sampleReport()
	after.Files = nil
	after.Functions = nil

	result, err := Compare(&before, &after)
	if err != nil {
		t.Fatal(err)
	}

	if len(result.FunctionChanges) != 1 || result.FunctionChanges[0].State != ComparisonDeletedFunction {
		t.Fatalf("removed function = %+v", result.FunctionChanges)
	}
}

func TestComparePreservesReviewDecisionAndAccounting(t *testing.T) {
	before := sampleReport()
	before.Findings[0].Fingerprint = "finding-source"
	ledger := report.InitializeReview(&before)
	for index := range ledger.Items {
		if ledger.Items[index].Kind != report.ReviewFinding {
			continue
		}
		ledger.Items[index].Status = report.ReviewRefactored
		ledger.Items[index].ReasonCategory = "cleanup"
		ledger.Items[index].Reason = "simplified branch"
		ledger.Items[index].Evidence = "reviewed source"
	}
	if err := report.ApplyReview(&before, &ledger); err != nil {
		t.Fatal(err)
	}
	after := before
	after.Findings = nil
	after.Review = nil

	result, err := Compare(&before, &after)
	if err != nil {
		t.Fatal(err)
	}
	if result.BeforeReview.Counts.Refactored != 1 || result.BeforeReview.Counts.Total != 1 {
		t.Fatalf("before review accounting = %+v", result.BeforeReview.Counts)
	}
	if len(result.FindingChanges) != 1 || result.FindingChanges[0].State != ComparisonNoLongerReported ||
		result.FindingChanges[0].BeforeReview == nil ||
		result.FindingChanges[0].BeforeReview.Status != report.ReviewRefactored ||
		result.FindingChanges[0].BeforeReview.Reason != "simplified branch" {
		t.Fatalf("review decision was lost: %+v", result.FindingChanges)
	}
}

func TestCompareRequestedDuplicationFailureIsIncomplete(t *testing.T) {
	before := sampleReport()
	after := sampleReport()
	before.Config.Duplication = before.Duplication.Config
	after.Config.Duplication = before.Config.Duplication
	after.Duplication = &report.Duplication{
		Status: report.DuplicationError,
		Error:  "tool failed",
		Config: before.Config.Duplication,
	}
	result, err := Compare(&before, &after)
	if err != nil {
		t.Fatal(err)
	}
	if result.Compatible || result.Complete || result.MetricsDelta != nil ||
		!strings.Contains(result.Reason, "duplication") {
		t.Fatalf("failed requested duplication looked complete: %+v", result)
	}
}

func TestCompareDuplicationMissingOrFailedNeverLooksLikeZero(t *testing.T) {
	before := sampleReport()
	after := sampleReport()
	before.Duplication = nil
	after.Duplication = &report.Duplication{Status: report.DuplicationError, Error: "cpd failed"}
	result, err := Compare(&before, &after)
	if err != nil {
		t.Fatal(err)
	}

	if result.Duplication.Comparable || result.Duplication.BeforeStatus != report.DuplicationNotRequested ||
		result.Duplication.AfterStatus != report.DuplicationError {
		t.Fatalf("duplication was treated as comparable: %+v", result.Duplication)
	}
}

func TestCompareRejectsUnsupportedSchema(t *testing.T) {
	value := sampleReport()
	value.SchemaVersion = "999"
	other := sampleReport()
	if _, err := Compare(&value, &other); err == nil {
		t.Fatal("unsupported schema was accepted")
	}
}

func sampleReport() report.Report {
	share := .2
	return report.Report{
		SchemaVersion: report.SchemaVersion,
		Analyzer:      report.AnalyzerIdentity{ID: "native-go", Version: "1"},
		Config:        report.ConfigSummary{Language: "go", HighComplexityThreshold: 10},
		Scope:         report.Scope{Languages: []string{"go"}},
		Coverage:      report.Coverage{Discovered: 1, Analyzed: 1},
		Findings: []report.Finding{
			{
				ID:       "finding-1",
				Rule:     "style",
				Path:     "main.go",
				Start:    report.Position{Line: 2, Column: 1},
				End:      report.Position{Line: 2, Column: 2},
				Severity: "warning",
				Message:  "simplify return",
			},
		},
		Metrics: report.Metrics{
			All: report.MetricBucket{
				CodeLines:               10,
				Functions:               1,
				Cyclomatic:              2,
				Mass:                    4,
				HighComplexityMassShare: &share,
			},
		},
		Duplication: &report.Duplication{
			Status: report.DuplicationMeasured,
			Config: report.DuplicationConfig{Requested: true, Tool: "cpd", Version: "5.3.0"},
		},
	}
}
