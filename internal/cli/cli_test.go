package cli

import (
	"bytes"
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"github.com/kellen-miller/deburr/internal/report"
)

func TestRunAuditJSONKeepsStdoutMachineReadableAndFindingsAdvisory(t *testing.T) {
	root := t.TempDir()
	writeFile(t, filepath.Join(root, "main.go"), `package sample

func Check(value int) bool {
	if value > 0 {
		return true
	}
	return false
}
`)
	var stdout, stderr bytes.Buffer
	code := Run(t.Context(), []string{"audit", root, "--format", "json"}, &stdout, &stderr)
	if code != 0 {
		t.Fatalf("exit = %d, stderr = %s", code, stderr.String())
	}

	if stderr.Len() != 0 {
		t.Fatalf("stderr = %q", stderr.String())
	}

	var value report.Report
	if err := json.Unmarshal(stdout.Bytes(), &value); err != nil {
		t.Fatalf("stdout is not JSON: %v\n%s", err, stdout.String())
	}

	if len(value.Findings) == 0 {
		t.Fatal("fixture did not produce an advisory finding")
	}

	if value.Review == nil || value.Review.Counts.Unreviewed == 0 {
		t.Fatalf("review accounting = %+v", value.Review)
	}
}

func TestRunAuditParseFailureEmitsReportAndError(t *testing.T) {
	root := t.TempDir()
	writeFile(t, filepath.Join(root, "broken.go"), "package broken\nfunc (")
	var stdout, stderr bytes.Buffer
	code := Run(t.Context(), []string{"audit", "--format=json", root}, &stdout, &stderr)
	if code == 0 {
		t.Fatal("parse failure returned success")
	}

	if !strings.Contains(stderr.String(), "analysis failed") {
		t.Fatalf("stderr = %q", stderr.String())
	}

	var value report.Report
	if err := json.Unmarshal(stdout.Bytes(), &value); err != nil {
		t.Fatalf("partial report is not JSON: %v\n%s", err, stdout.String())
	}

	if value.Coverage.ParseErrors != 1 {
		t.Fatalf("coverage = %+v", value.Coverage)
	}
}

func TestRunRejectsOutputThatCouldOverwriteSource(t *testing.T) {
	root := t.TempDir()
	source := filepath.Join(root, "main.go")
	writeFile(t, source, "package sample\n")
	original, err := os.ReadFile(source)
	if err != nil {
		t.Fatal(err)
	}

	var stdout, stderr bytes.Buffer
	code := Run(t.Context(), []string{"audit", root, "--format", "json", "--output", source}, &stdout, &stderr)
	if code == 0 || !strings.Contains(stderr.String(), "outside the audit target") {
		t.Fatalf("exit=%d stderr=%q", code, stderr.String())
	}

	after, err := os.ReadFile(source)
	if err != nil {
		t.Fatal(err)
	}

	if !bytes.Equal(original, after) {
		t.Fatal("source changed after rejected output")
	}
}

func TestRunRejectsAnyOutputInsideAuditTarget(t *testing.T) {
	root := t.TempDir()
	writeFile(t, filepath.Join(root, "main.go"), "package sample\n")
	var stdout, stderr bytes.Buffer
	code := Run(
		t.Context(),
		[]string{"audit", root, "--format", "json", "--output", filepath.Join(root, "go.mod")},
		&stdout,
		&stderr,
	)
	if code == 0 || !strings.Contains(stderr.String(), "outside the audit target") {
		t.Fatalf("exit=%d stderr=%q", code, stderr.String())
	}
}

func TestRunRejectsOutputThroughParentSymlink(t *testing.T) {
	root := t.TempDir()
	writeFile(t, filepath.Join(root, "main.go"), "package sample\n")
	parent := filepath.Join(t.TempDir(), "alias")
	if err := os.Symlink(root, parent); err != nil {
		t.Skipf("symlinks unavailable: %v", err)
	}

	var stdout, stderr bytes.Buffer
	code := Run(
		t.Context(),
		[]string{"audit", root, "--format", "json", "--output", filepath.Join(parent, "report.json")},
		&stdout,
		&stderr,
	)
	if code == 0 || !strings.Contains(stderr.String(), "outside the audit target") {
		t.Fatalf("exit=%d stderr=%q", code, stderr.String())
	}
}

func TestRunRejectsOutputHardlinkToSource(t *testing.T) {
	root := t.TempDir()
	source := filepath.Join(root, "main.go")
	alias := filepath.Join(root, "report.json")
	writeFile(t, source, "package sample\n")
	if err := os.Link(source, alias); err != nil {
		t.Skipf("hard links unavailable: %v", err)
	}

	var stdout, stderr bytes.Buffer
	code := Run(t.Context(), []string{"audit", root, "--format", "json", "--output", alias}, &stdout, &stderr)
	if code == 0 || !strings.Contains(stderr.String(), "outside the audit target") {
		t.Fatalf("exit=%d stderr=%q", code, stderr.String())
	}

	sourceData, err := os.ReadFile(source)
	if err != nil {
		t.Fatal(err)
	}

	if string(sourceData) != "package sample\n" {
		t.Fatalf("source was changed through hardlink output: %q", sourceData)
	}
}

func TestRunCompareCompatibleAndInvalidReports(t *testing.T) {
	root := t.TempDir()
	before := filepath.Join(root, "before.json")
	after := filepath.Join(root, "after.json")
	writeJSON(t, before, comparisonReport(10))
	writeJSON(t, after, comparisonReport(12))
	var stdout, stderr bytes.Buffer
	code := Run(t.Context(), []string{"compare", before, after, "--format=json"}, &stdout, &stderr)
	if code != 0 {
		t.Fatalf("compatible compare exit=%d stderr=%q", code, stderr.String())
	}

	if !strings.Contains(stdout.String(), `"compatible": true`) ||
		!strings.Contains(stdout.String(), `"code_lines": 2`) {
		t.Fatalf("comparison output = %s", stdout.String())
	}

	stdout.Reset()
	stderr.Reset()
	writeFile(t, after, "not json")
	code = Run(t.Context(), []string{"compare", before, after, "--format", "json"}, &stdout, &stderr)
	if code == 0 || stdout.Len() != 0 || !strings.Contains(stderr.String(), "decode") {
		t.Fatalf("invalid compare exit=%d stdout=%q stderr=%q", code, stdout.String(), stderr.String())
	}
}

func TestRunCompareRefusesInputOverwrite(t *testing.T) {
	root := t.TempDir()
	before := filepath.Join(root, "before.json")
	after := filepath.Join(root, "after.json")
	writeJSON(t, before, comparisonReport(10))
	writeJSON(t, after, comparisonReport(12))
	var stdout, stderr bytes.Buffer
	code := Run(
		t.Context(),
		[]string{"compare", before, after, "--format", "json", "--output", before},
		&stdout,
		&stderr,
	)
	if code == 0 || stdout.Len() != 0 || !strings.Contains(stderr.String(), "overwrite input") {
		t.Fatalf("exit=%d stdout=%q stderr=%q", code, stdout.String(), stderr.String())
	}
}

func TestRunGitHubNestedRootUsesRepositoryRelativePaths(t *testing.T) {
	workspace := t.TempDir()
	root := filepath.Join(workspace, "backend")
	writeFile(t, filepath.Join(root, "main.go"), `package sample

func Check(value int) bool {
	if value > 0 {
		return true
	}
	return false
}
`)
	t.Setenv("GITHUB_WORKSPACE", workspace)
	var stdout, stderr bytes.Buffer
	code := Run(t.Context(), []string{"audit", root, "--format", "github"}, &stdout, &stderr)
	if code != 0 {
		t.Fatalf("exit=%d stderr=%q", code, stderr.String())
	}

	if !strings.Contains(stdout.String(), "file=backend/main.go") {
		t.Fatalf("GitHub output did not prefix nested root: %s", stdout.String())
	}
}

func TestRunOutputDirectoryErrorIsCaptured(t *testing.T) {
	root := t.TempDir()
	writeFile(t, filepath.Join(root, "main.go"), "package sample\n")
	output := filepath.Join(t.TempDir(), "missing", "report.json")
	var stdout, stderr bytes.Buffer
	code := Run(t.Context(), []string{"audit", root, "--format", "json", "--output", output}, &stdout, &stderr)
	if code == 0 || stdout.Len() != 0 || !strings.Contains(stderr.String(), "open output") {
		t.Fatalf("exit=%d stdout=%q stderr=%q", code, stdout.String(), stderr.String())
	}
}

func TestRunReviewInitAndApplyAccountsMissingAndStaleItems(t *testing.T) {
	root := t.TempDir()
	reportPath := filepath.Join(root, "baseline.json")
	ledgerPath := filepath.Join(root, "review.json")
	annotatedPath := filepath.Join(root, "annotated.json")
	baseline := reviewCLIReport()
	writeJSON(t, reportPath, baseline)

	var stdout, stderr bytes.Buffer
	code := Run(
		t.Context(),
		[]string{"review", "init", reportPath, "--output", ledgerPath},
		&stdout,
		&stderr,
	)
	if code != 0 {
		t.Fatalf("review init exit=%d stderr=%q", code, stderr.String())
	}

	ledger := readLedger(t, ledgerPath)
	if ledger.Counts != (report.ReviewCounts{Total: 3, Unreviewed: 3}) {
		t.Fatalf("initial review counts = %+v", ledger.Counts)
	}

	for index := range ledger.Items {
		switch ledger.Items[index].ID {
		case "finding-1":
			ledger.Items[index].Status = report.ReviewRetained
			ledger.Items[index].ReasonCategory = "semantics"
			ledger.Items[index].Reason = "condition is part of the public contract"
			ledger.Items[index].Evidence = "reviewed source and behavior test"
		case "finding-2":
			ledger.Items[index].Status = report.ReviewRefactored
			ledger.Items[index].ReasonCategory = "maintenance"
			ledger.Items[index].Reason = "removed repeated branch"
			ledger.Items[index].Evidence = "commit abc123 and focused test"
		}
	}
	writeLedgerJSON(t, ledgerPath, ledger)

	changed := baseline
	changed.Findings[1].Fingerprint = "finding-2-after"
	writeJSON(t, reportPath, changed)
	stdout.Reset()
	stderr.Reset()
	code = Run(
		t.Context(),
		[]string{"review", "apply", reportPath, "--ledger", ledgerPath, "--format", "json", "--output", annotatedPath},
		&stdout,
		&stderr,
	)
	if code != 0 {
		t.Fatalf("review apply exit=%d stderr=%q", code, stderr.String())
	}

	annotated := readReport(t, annotatedPath)
	if annotated.Review == nil {
		t.Fatal("review apply omitted report ledger")
	}

	wantCounts := report.ReviewCounts{Total: 3, Unreviewed: 2, Retained: 1, Stale: 1}
	if annotated.Review.Counts != wantCounts {
		t.Fatalf("applied review counts = %+v, want %+v", annotated.Review.Counts, wantCounts)
	}

	for index := range annotated.Review.Items {
		item := annotated.Review.Items[index]
		if item.ID == "finding-2" {
			if item.Status != report.ReviewUnreviewed || item.Previous == nil ||
				item.Previous.Status != report.ReviewRefactored {
				t.Fatalf("stale decision = %+v", item)
			}
		}
	}
}

func TestRunReviewApplyRejectsUnknownAndMismatchedReports(t *testing.T) {
	root := t.TempDir()
	reportPath := filepath.Join(root, "report.json")
	ledgerPath := filepath.Join(root, "review.json")
	value := reviewCLIReport()
	writeJSON(t, reportPath, value)

	ledger := report.InitializeReview(&value)
	ledger.Items = append(ledger.Items, report.ReviewItem{
		ID:          "unknown",
		Kind:        report.ReviewFinding,
		Fingerprint: "unknown",
		Status:      report.ReviewUnreviewed,
	})
	writeLedgerJSON(t, ledgerPath, ledger)

	var stdout, stderr bytes.Buffer
	code := Run(
		t.Context(),
		[]string{"review", "apply", reportPath, "--ledger", ledgerPath},
		&stdout,
		&stderr,
	)
	if code == 0 || !strings.Contains(stderr.String(), "unknown id") {
		t.Fatalf("unknown review item exit=%d stderr=%q", code, stderr.String())
	}

	ledger = report.InitializeReview(&value)
	writeLedgerJSON(t, ledgerPath, ledger)
	value.Config.Language = "other"
	writeJSON(t, reportPath, value)
	stdout.Reset()
	stderr.Reset()
	code = Run(
		t.Context(),
		[]string{"review", "apply", reportPath, "--ledger", ledgerPath},
		&stdout,
		&stderr,
	)
	if code == 0 || !strings.Contains(stderr.String(), "report_identity") {
		t.Fatalf("mismatched review exit=%d stderr=%q", code, stderr.String())
	}
}

func TestRunReviewRejectsOutputOverInput(t *testing.T) {
	root := t.TempDir()
	reportPath := filepath.Join(root, "report.json")
	ledgerPath := filepath.Join(root, "review.json")
	writeJSON(t, reportPath, reviewCLIReport())

	var stdout, stderr bytes.Buffer
	code := Run(
		t.Context(),
		[]string{"review", "init", reportPath, "--output", reportPath},
		&stdout,
		&stderr,
	)
	if code == 0 || !strings.Contains(stderr.String(), "overwrite input") {
		t.Fatalf("review init overwrite exit=%d stderr=%q", code, stderr.String())
	}

	value := reviewCLIReport()
	writeJSON(t, reportPath, value)
	writeLedgerJSON(t, ledgerPath, report.InitializeReview(&value))
	stdout.Reset()
	stderr.Reset()
	code = Run(
		t.Context(),
		[]string{"review", "apply", reportPath, "--ledger", ledgerPath, "--output", ledgerPath},
		&stdout,
		&stderr,
	)
	if code == 0 || !strings.Contains(stderr.String(), "overwrite input") {
		t.Fatalf("review apply overwrite exit=%d stderr=%q", code, stderr.String())
	}
}

func TestCurrentSourceIdentitySeparatesCommitAndDirtyState(t *testing.T) {
	if _, err := exec.LookPath("git"); err != nil {
		t.Skipf("git unavailable: %v", err)
	}

	root := t.TempDir()
	writeFile(t, filepath.Join(root, "main.go"), "package sample\n")
	runTestGit(t, root, "init", "--quiet")
	runTestGit(t, root, "config", "user.email", "deburr@example.invalid")
	runTestGit(t, root, "config", "user.name", "Deburr Test")
	runTestGit(t, root, "add", "main.go")
	runTestGit(t, root, "commit", "--quiet", "-m", "initial")

	clean := currentSourceIdentity(root)
	if !clean.GitKnown || clean.Commit == "" || clean.Dirty {
		t.Fatalf("clean source identity = %+v", clean)
	}

	writeFile(t, filepath.Join(root, "main.go"), "package changed\n")
	dirty := currentSourceIdentity(root)
	if !dirty.GitKnown || dirty.Commit != clean.Commit || !dirty.Dirty {
		t.Fatalf("dirty source identity = %+v, clean = %+v", dirty, clean)
	}
}

func comparisonReport(codeLines int) report.Report {
	return report.Report{
		SchemaVersion: report.SchemaVersion,
		Analyzer:      report.AnalyzerIdentity{ID: "native-go", Version: "1"},
		Config:        report.ConfigSummary{Language: "go"},
		Scope:         report.Scope{Languages: []string{"go"}},
		Coverage:      report.Coverage{Discovered: 1, Analyzed: 1},
		Findings: []report.Finding{
			{
				ID:       "finding-1",
				Rule:     "style",
				Path:     "main.go",
				Start:    report.Position{Line: 1},
				End:      report.Position{Line: 1},
				Severity: "review",
				Message:  "review",
			},
		},
		Metrics: report.Metrics{All: report.MetricBucket{CodeLines: codeLines}},
	}
}

func reviewCLIReport() report.Report {
	return report.Report{
		SchemaVersion: report.SchemaVersion,
		Analyzer:      report.AnalyzerIdentity{ID: "native-go", Version: "1"},
		Config:        report.ConfigSummary{Language: "go"},
		Scope:         report.Scope{Languages: []string{"go"}},
		Coverage:      report.Coverage{Discovered: 1, Analyzed: 1},
		Findings: []report.Finding{
			{
				ID:          "finding-1",
				Fingerprint: "finding-1-before",
				Rule:        "style",
				Path:        "main.go",
				Severity:    "review",
				Message:     "first",
				Start:       report.Position{Line: 1, Column: 1},
				End:         report.Position{Line: 1, Column: 2},
			},
			{
				ID:          "finding-2",
				Fingerprint: "finding-2-before",
				Rule:        "style",
				Path:        "main.go",
				Severity:    "review",
				Message:     "second",
				Start:       report.Position{Line: 2, Column: 1},
				End:         report.Position{Line: 2, Column: 2},
			},
			{
				ID:          "finding-3",
				Fingerprint: "finding-3-before",
				Rule:        "style",
				Path:        "main.go",
				Severity:    "review",
				Message:     "third",
				Start:       report.Position{Line: 3, Column: 1},
				End:         report.Position{Line: 3, Column: 2},
			},
		},
	}
}

func readReport(t *testing.T, path string) report.Report {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}

	var value report.Report
	if err := json.Unmarshal(data, &value); err != nil {
		t.Fatal(err)
	}

	return value
}

func readLedger(t *testing.T, path string) report.ReviewLedger {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}

	var ledger report.ReviewLedger
	if err := json.Unmarshal(data, &ledger); err != nil {
		t.Fatal(err)
	}

	return ledger
}

func writeLedgerJSON(t *testing.T, path string, value report.ReviewLedger) {
	t.Helper()
	data, err := json.Marshal(value)
	if err != nil {
		t.Fatal(err)
	}

	writeFile(t, path, string(data))
}

func runTestGit(t *testing.T, directory string, args ...string) {
	t.Helper()
	commandArgs := append([]string{"-C", directory}, args...)
	if output, err := exec.Command("git", commandArgs...).CombinedOutput(); err != nil {
		t.Fatalf("git %v: %v\n%s", args, err, output)
	}
}

func writeFile(t *testing.T, path, contents string) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatal(err)
	}

	if err := os.WriteFile(path, []byte(contents), 0o644); err != nil {
		t.Fatal(err)
	}
}

func writeJSON(t *testing.T, path string, value report.Report) {
	t.Helper()
	data, err := json.Marshal(value)
	if err != nil {
		t.Fatal(err)
	}

	writeFile(t, path, string(data))
}
