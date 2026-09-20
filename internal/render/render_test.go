package render

import (
	"bytes"
	"errors"
	"fmt"
	"strings"
	"testing"

	"github.com/kellen-miller/deburr/internal/compare"
	"github.com/kellen-miller/deburr/internal/report"
)

func TestJSONIsDeterministicAndRoundTrips(t *testing.T) {
	value := sampleReport()
	first, err := MarshalJSON(&value)
	if err != nil {
		t.Fatal(err)
	}

	second, err := MarshalJSON(&value)
	if err != nil {
		t.Fatal(err)
	}

	if !bytes.Equal(first, second) {
		t.Fatal("JSON rendering changed between identical renders")
	}

	decoded, err := DecodeJSON(first)
	if err != nil {
		t.Fatal(err)
	}

	if decoded.Findings[0].Message != value.Findings[0].Message {
		t.Fatalf("round trip message = %q", decoded.Findings[0].Message)
	}

	if strings.Contains(string(first), "/Users/") || strings.Contains(string(first), "timestamp") {
		t.Fatalf("JSON contains an unstable value: %s", first)
	}
}

func TestHTMLEscapesReportValuesAndIncludesEvidence(t *testing.T) {
	value := sampleReport()
	var output bytes.Buffer
	if err := Write(&output, &value, FormatHTML); err != nil {
		t.Fatal(err)
	}

	content := output.String()
	if strings.Contains(content, "<b>bad</b>") || strings.Contains(content, `data-search="<script>`) {
		t.Fatalf("HTML did not escape report values: %s", content)
	}

	for _, expected := range []string{"Function inventory (1)", "Function hotspots", "Duplication", "clone-1", "&lt;script&gt;"} {
		if !strings.Contains(content, expected) {
			t.Errorf("HTML missing %q", expected)
		}
	}
}

func TestHTMLFormatsNullableSharesDeterministically(t *testing.T) {
	firstValue := sampleReport()
	secondValue := sampleReport()
	var first bytes.Buffer
	var second bytes.Buffer
	if err := Write(&first, &firstValue, FormatHTML); err != nil {
		t.Fatal(err)
	}

	if err := Write(&second, &secondValue, FormatHTML); err != nil {
		t.Fatal(err)
	}

	if !bytes.Equal(first.Bytes(), second.Bytes()) {
		t.Fatal("HTML rendering changed between separately allocated equal reports")
	}

	content := first.String()
	for _, expected := range []string{"0.250000", "null"} {
		if !strings.Contains(content, expected) {
			t.Errorf("HTML missing nullable share value %q", expected)
		}
	}

	if strings.Contains(content, "%!f") {
		t.Fatalf("HTML formatted a share pointer: %s", content)
	}
}

func TestHTMLIncludesCompleteRankedReviewInventoryAndFilters(t *testing.T) {
	value := sampleReport()
	value.Functions = make([]report.Function, 25)
	for index := range value.Functions {
		value.Functions[index] = report.Function{
			ID:             fmt.Sprintf("function-%02d", index),
			Path:           "main.go",
			Category:       report.CategoryProduction,
			Name:           fmt.Sprintf("Function%02d", index),
			Start:          report.Position{Line: index + 1, Column: 1},
			End:            report.Position{Line: index + 2, Column: 1},
			SLOC:           index + 1,
			Cyclomatic:     11,
			MaxNesting:     1,
			Mass:           float64(25 - index),
			HighComplexity: index < 21,
		}
	}

	var output bytes.Buffer
	if err := Write(&output, &value, FormatHTML); err != nil {
		t.Fatal(err)
	}

	content := output.String()
	for _, expected := range []string{
		"Function hotspots (21 high-complexity of 25 functions)",
		"Function inventory (25)",
		"native findings (1)",
		"Duplication clones (1)",
		"id=\"inventory-search\"",
		"id=\"inventory-view\"",
		"data-review-row",
		"data-review=\"false\"",
		"row.dataset.search.toLowerCase()",
		"row.hidden = !matches",
		"function-24",
	} {
		if !strings.Contains(content, expected) {
			t.Errorf("HTML missing %q", expected)
		}
	}

	first := strings.Index(content, "function-00")
	last := strings.Index(content, "function-24")
	if first < 0 || last < 0 || first >= last {
		t.Fatalf("function rows are not fully ranked: function-00=%d function-24=%d", first, last)
	}

	if strings.Contains(content, "src=") {
		t.Fatal("HTML loaded an external asset")
	}
}

func TestHTMLUsesReviewLedgerDecisionFields(t *testing.T) {
	value := sampleReport()
	ledger := report.InitializeReview(&value)
	for index := range ledger.Items {
		if ledger.Items[index].Kind != report.ReviewFinding {
			continue
		}

		ledger.Items[index].Status = report.ReviewRetained
		ledger.Items[index].ReasonCategory = "contract"
		ledger.Items[index].Reason = "the native rule is intentional"
		ledger.Items[index].Evidence = "reviewed native finding and source"
		ledger.Items[index].Upstream = "go vet"
	}
	if err := report.ApplyReview(&value, &ledger); err != nil {
		t.Fatal(err)
	}

	var output bytes.Buffer
	if err := Write(&output, &value, FormatHTML); err != nil {
		t.Fatal(err)
	}

	content := output.String()
	for _, expected := range []string{
		"retained",
		"contract</code>: the native rule is intentional",
		"reviewed native finding and source",
		"go vet",
		"fp: finding-fingerprint",
	} {
		if !strings.Contains(content, expected) {
			t.Errorf("HTML missing review ledger value %q", expected)
		}
	}
}

func TestHTMLJoinsReviewDecisionsByNativeID(t *testing.T) {
	value := sampleReport()
	value.Findings = append(value.Findings, report.Finding{
		ID:          "finding-0",
		Fingerprint: "finding-zero-fingerprint",
		Rule:        "aaa",
		Path:        "main.go",
		Start:       value.Findings[0].Start,
		End:         value.Findings[0].End,
		Severity:    "warning",
		Message:     "second same-span finding",
	})
	ledger := report.InitializeReview(&value)
	for index := range ledger.Items {
		switch ledger.Items[index].ID {
		case "finding-0":
			ledger.Items[index].Status = report.ReviewRetained
			ledger.Items[index].ReasonCategory = "category-zero"
			ledger.Items[index].Reason = "reason-zero"
			ledger.Items[index].Evidence = "evidence-zero"
		case "finding-1":
			ledger.Items[index].Status = report.ReviewRetained
			ledger.Items[index].ReasonCategory = "category-one"
			ledger.Items[index].Reason = "reason-one"
			ledger.Items[index].Evidence = "evidence-one"
		}
	}
	for left, right := 0, len(ledger.Items)-1; left < right; left, right = left+1, right-1 {
		ledger.Items[left], ledger.Items[right] = ledger.Items[right], ledger.Items[left]
	}
	if err := report.ApplyReview(&value, &ledger); err != nil {
		t.Fatal(err)
	}

	var output bytes.Buffer
	if err := Write(&output, &value, FormatHTML); err != nil {
		t.Fatal(err)
	}

	content := output.String()
	firstID := strings.Index(content, "finding-0")
	firstReason := strings.Index(content, "reason-zero")
	secondID := strings.Index(content, "finding-1")
	secondReason := strings.Index(content, "reason-one")
	if firstID < 0 || firstReason < 0 || secondID < 0 || secondReason < 0 ||
		firstID >= firstReason || firstReason >= secondID || secondID >= secondReason {
		t.Fatalf(
			"same-span decisions were mismatched: id0=%d reason0=%d id1=%d reason1=%d",
			firstID,
			firstReason,
			secondID,
			secondReason,
		)
	}
}

func TestHTMLRejectsUnknownReviewItem(t *testing.T) {
	value := sampleReport()
	ledger := report.InitializeReview(&value)
	ledger.Items[0].ID = "unknown-review-item"
	value.Review = &ledger

	var output bytes.Buffer
	if err := Write(&output, &value, FormatHTML); err == nil {
		t.Fatal("HTML rendering accepted an unknown review item")
	}
}

func TestTextAndGitHubIncludeReviewAccounting(t *testing.T) {
	value := sampleReport()
	ledger := report.InitializeReview(&value)
	for index := range ledger.Items {
		if ledger.Items[index].Kind != report.ReviewFinding {
			continue
		}

		ledger.Items[index].Status = report.ReviewRefactored
		ledger.Items[index].ReasonCategory = "cleanup"
		ledger.Items[index].Reason = "rule handled by shared path"
		ledger.Items[index].Evidence = "render test"
	}
	if err := report.ApplyReview(&value, &ledger); err != nil {
		t.Fatal(err)
	}

	var textOutput bytes.Buffer
	if err := Write(&textOutput, &value, FormatText); err != nil {
		t.Fatal(err)
	}
	for _, expected := range []string{
		"Review: total=2 unreviewed=1 refactored=1",
		"finding finding-1 [refactored]",
		"reason=rule handled by shared path",
		"evidence=render test",
	} {
		if !strings.Contains(textOutput.String(), expected) {
			t.Errorf("text output missing review value %q", expected)
		}
	}

	var githubOutput bytes.Buffer
	if err := Write(&githubOutput, &value, FormatGitHub); err != nil {
		t.Fatal(err)
	}
	for _, expected := range []string{
		"title=deburr review::total=2,unreviewed=1,refactored=1",
		"finding finding-1 status=refactored",
		"reason=rule handled by shared path",
	} {
		if !strings.Contains(githubOutput.String(), expected) {
			t.Errorf("GitHub output missing review value %q", expected)
		}
	}
}

func TestGitHubEscapesPathAndMessageAndReportsCoverage(t *testing.T) {
	value := sampleReport()
	value.Findings[0].Path = "dir,a\nb.go"
	value.Findings[0].Message = "bad:%\nnext"
	var output bytes.Buffer
	if err := Write(&output, &value, FormatGitHub); err != nil {
		t.Fatal(err)
	}

	content := output.String()
	for _, expected := range []string{"file=dir%2Ca%0Ab.go", "bad:%25%0Anext", "title=style"} {
		if !strings.Contains(content, expected) {
			t.Errorf("GitHub output missing escaped %q: %s", expected, content)
		}
	}

	if !strings.Contains(content, "coverage") {
		t.Fatal("GitHub output omitted coverage summary")
	}
}

func TestGitHubReportsFileFailuresAndCloneLocations(t *testing.T) {
	value := sampleReport()
	value.Files = []report.File{{Path: "bad,file.go", Status: report.FileParseError, Error: "unexpected: token"}}
	value.Duplication.Clones[0].Locations = []report.Location{
		{Path: "copy,file.go", Start: report.Position{Line: 4}, End: report.Position{Line: 8}},
	}
	var output bytes.Buffer
	if err := Write(&output, &value, FormatGitHub); err != nil {
		t.Fatal(err)
	}

	content := output.String()
	for _, expected := range []string{"file=bad%2Cfile.go", "unexpected: token", "file=copy%2Cfile.go,line=4,endLine=8", "clone clone-1"} {
		if !strings.Contains(content, expected) {
			t.Errorf("GitHub output missing %q: %s", expected, content)
		}
	}
}

func TestRenderReportsWriterError(t *testing.T) {
	errWriter := failingWriter{}
	value := sampleReport()
	if err := Write(errWriter, &value, FormatText); err == nil {
		t.Fatal("text render succeeded with a failing writer")
	}

	if err := Write(errWriter, &value, FormatJSON); err == nil {
		t.Fatal("JSON render succeeded with a failing writer")
	}
}

func TestComparisonRenderersShowTypedInventoryReviewAndProvenance(t *testing.T) {
	before := sampleReport()
	before.Functions[0].Fingerprint = "function-source"
	before.Functions[0].HighComplexity = true
	before.Functions[0].Cyclomatic = 11
	before.Functions[0].StructuralSignals = report.StructuralSignals{FlatGuards: 1, NestedBranches: 2}
	before.Duplication.Clones[0].FamilyID = "family-1"
	before.Provenance = report.Provenance{
		Build:  report.BuildIdentity{Revision: "build-before", Known: true},
		Source: report.SourceIdentity{Commit: "commit-before", GitKnown: true, ManifestDigest: "sha256:before"},
	}
	after := before
	after.Functions = append([]report.Function(nil), before.Functions...)
	after.Functions[0].HighComplexity = false
	after.Functions[0].Cyclomatic = 3
	after.Functions[0].StructuralSignals = report.StructuralSignals{FlatGuards: 2}
	after.Provenance = report.Provenance{
		Build:  report.BuildIdentity{Revision: "build-after", Known: true},
		Source: report.SourceIdentity{Commit: "commit-after", GitKnown: true, ManifestDigest: "sha256:after"},
	}
	result, err := compare.Compare(&before, &after)
	if err != nil {
		t.Fatal(err)
	}
	for _, format := range []Format{FormatText, FormatJSON, FormatHTML, FormatGitHub} {
		var output bytes.Buffer
		if err := WriteComparison(&output, &result, format); err != nil {
			t.Fatalf("format %s: %v", format, err)
		}
		content := output.String()
		for _, expected := range []string{"resolved_threshold", "family-1", "build-before", "sha256:after"} {
			if !strings.Contains(content, expected) {
				t.Errorf("format %s missing %q: %s", format, expected, content)
			}
		}
	}
}

func TestDecodeRejectsInvalidSchemaAndUnknownFields(t *testing.T) {
	if _, err := DecodeJSON(
		[]byte(
			`{"schema_version":"1","analyzer":{"id":"x","version":"1"},"config":{"language":"go"},"scope":{"languages":["go"]},"unexpected":true}`,
		),
	); err == nil {
		t.Fatal("unknown field was accepted")
	}

	if _, err := DecodeJSON([]byte(`{"schema_version":"999"}`)); err == nil {
		t.Fatal("unsupported schema was accepted")
	}
}

type failingWriter struct{}

func (failingWriter) Write([]byte) (int, error) {
	return 0, errors.New("write failed")
}

func sampleReport() report.Report {
	share := 0.25
	return report.Report{
		SchemaVersion: report.SchemaVersion,
		Analyzer:      report.AnalyzerIdentity{ID: "native-go", Version: "1"},
		Config:        report.ConfigSummary{Language: "go"},
		Scope:         report.Scope{Languages: []string{"go"}},
		Coverage:      report.Coverage{Discovered: 1, Analyzed: 1},
		Files: []report.File{
			{
				Path:      "main.go",
				Category:  report.CategoryProduction,
				Status:    report.FileAnalyzed,
				Bytes:     20,
				Lines:     4,
				CodeLines: 2,
			},
		},
		Functions: []report.Function{
			{
				ID:         "function-1",
				Path:       "main.go",
				Category:   report.CategoryProduction,
				Name:       "<script>",
				Start:      report.Position{Line: 1, Column: 1},
				End:        report.Position{Line: 4, Column: 1},
				SLOC:       2,
				Cyclomatic: 2,
				MaxNesting: 1,
				Mass:       2.8,
			},
		},
		Findings: []report.Finding{
			{
				ID:          "finding-1",
				Fingerprint: "finding-fingerprint",
				Rule:        "style",
				Path:        "main.go",
				Start:       report.Position{Line: 2, Column: 1},
				End:         report.Position{Line: 2, Column: 4},
				Severity:    "warning",
				Message:     "<b>bad</b>",
			},
		},
		Metrics: report.Metrics{
			All: report.MetricBucket{
				CodeLines:               2,
				Functions:               1,
				Cyclomatic:              2,
				MaxNesting:              1,
				Mass:                    2.8,
				HighComplexityMassShare: &share,
			},
		},
		Duplication: &report.Duplication{
			Status: report.DuplicationMeasured,
			Config: report.DuplicationConfig{Requested: true, Tool: "cpd", Version: "5.3.0"},
			Clones: []report.Clone{{
				ID: "clone-1", Fingerprint: "clone-fingerprint", Tokens: 10, Lines: 3,
			}},
		},
	}
}
