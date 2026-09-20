package analysis

import (
	"bytes"
	"encoding/json"
	"errors"
	"go/parser"
	"go/token"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"github.com/kellen-miller/deburr/internal/report"
)

func TestAnalyzeMetricsFindingsAndNestedClosures(t *testing.T) {
	root := t.TempDir()
	writeTestFile(t, root, "sample.go", `package sample

func Outer(value int) bool {
	if value > 0 {
		return true
	}
	return false
}

func Nested(value int) int {
	if value > 0 && value < 10 {
		inner := func() int {
			if value == 4 {
				return 1
			}
			return 2
		}
		return inner()
	}
	return 0
}
`)

	result, err := Analyze(t.Context(), root, DefaultConfig())
	if err != nil {
		t.Fatalf("Analyze() error = %v", err)
	}

	if result.Coverage.Analyzed != 1 || result.Coverage.ParseErrors != 0 {
		t.Fatalf("coverage = %+v", result.Coverage)
	}

	if len(result.Functions) != 3 {
		t.Fatalf("function count = %d, want 3", len(result.Functions))
	}

	if result.Functions[0].Name != "Outer" || result.Functions[1].Name != "Nested" ||
		result.Functions[2].Name != "Nested.func1" {
		for index := range result.Functions {
			t.Logf("function: %+v", result.Functions[index])
		}

		t.Fatalf("function ordering/names unexpected")
	}

	outer := result.Functions[0]
	if outer.Cyclomatic != 2 || outer.MaxNesting != 1 || outer.SLOC != 6 {
		t.Fatalf("outer metrics = %+v", outer)
	}

	nested := result.Functions[1]
	if nested.Cyclomatic != 3 || nested.MaxNesting != 1 {
		t.Fatalf("nested metrics = %+v", nested)
	}

	if nested.SLOC != 6 || result.Functions[2].SLOC != 6 {
		t.Fatalf("closure/parent SLOC = %d/%d, want 6/6", nested.SLOC, result.Functions[2].SLOC)
	}

	if result.Metrics.All.Functions != 3 || result.Metrics.All.Cyclomatic != 7 || result.Metrics.All.Mass == 0 {
		t.Fatalf("aggregate metrics = %+v", result.Metrics.All)
	}

	if result.Metrics.All.HighComplexityMassShare == nil || *result.Metrics.All.HighComplexityMassShare != 0 {
		t.Fatalf("high complexity share = %v, want 0", result.Metrics.All.HighComplexityMassShare)
	}

	if len(result.Findings) != 1 || result.Findings[0].Rule != "redundant-boolean-return" {
		t.Fatalf("findings = %+v", result.Findings)
	}
}

func TestAnalyzeFunctionIdentityAndFingerprintIgnoreLineMoves(t *testing.T) {
	left := t.TempDir()
	right := t.TempDir()
	writeTestFile(t, left, "sample.go", `package sample

type Left struct{}
type Right struct{}

func (Left) Run(value int) bool {
	if value > 0 {
		return true
	}
	return false
}

func (Right) Run(value int) bool {
	if value > 0 {
		return true
	}
	return false
}
`)
	writeTestFile(t, right, "sample.go", `package sample

type Left struct{}
type Right struct{}


// line insertion must not change stable source identity

func (Left) Run(value int) bool {
	if value > 0 {
		return true
	}
	return false
}

func (Right) Run(value int) bool {
	if value > 0 {
		return true
	}
	return false
}
`)

	leftReport, err := Analyze(t.Context(), left, DefaultConfig())
	if err != nil {
		t.Fatalf("left Analyze() error = %v", err)
	}
	rightReport, err := Analyze(t.Context(), right, DefaultConfig())
	if err != nil {
		t.Fatalf("right Analyze() error = %v", err)
	}

	if len(leftReport.Functions) != len(rightReport.Functions) || len(leftReport.Functions) != 2 {
		t.Fatalf("functions = %d/%d, want two in each report", len(leftReport.Functions), len(rightReport.Functions))
	}
	for index := range leftReport.Functions {
		leftFunction := leftReport.Functions[index]
		rightFunction := rightReport.Functions[index]
		if leftFunction.ID != rightFunction.ID {
			t.Fatalf(
				"function %q IDs differ after line insertion: %q/%q",
				leftFunction.Name,
				leftFunction.ID,
				rightFunction.ID,
			)
		}
		if leftFunction.Fingerprint != rightFunction.Fingerprint {
			t.Fatalf("function %q fingerprints differ after line insertion", leftFunction.Name)
		}
	}
	if leftReport.Functions[0].ID == leftReport.Functions[1].ID {
		t.Fatalf("receiver-qualified functions collided: %+v", leftReport.Functions)
	}

	if len(leftReport.Findings) != len(rightReport.Findings) || len(leftReport.Findings) != 2 {
		t.Fatalf("findings = %d/%d, want two in each report", len(leftReport.Findings), len(rightReport.Findings))
	}
	for index := range leftReport.Findings {
		if leftReport.Findings[index].ID != rightReport.Findings[index].ID {
			t.Fatalf(
				"finding IDs differ after line insertion: %q/%q",
				leftReport.Findings[index].ID,
				rightReport.Findings[index].ID,
			)
		}
		if leftReport.Findings[index].Fingerprint != rightReport.Findings[index].Fingerprint {
			t.Fatalf("finding fingerprints differ after line insertion")
		}
	}
}

func TestAnalyzeDuplicateInitFunctionsHaveDistinctStableIDs(t *testing.T) {
	left := t.TempDir()
	right := t.TempDir()
	writeTestFile(t, left, "init.go", `package sample

func init() {
	if true {
		return
	}
}

func init() {
	if true {
		return
	}
}
`)
	writeTestFile(t, right, "init.go", `package sample


func init() {
	if true {
		return
	}
}

func init() {
	if true {
		return
	}
}
`)

	leftReport, err := Analyze(t.Context(), left, DefaultConfig())
	if err != nil {
		t.Fatalf("left Analyze() error = %v", err)
	}
	rightReport, err := Analyze(t.Context(), right, DefaultConfig())
	if err != nil {
		t.Fatalf("right Analyze() error = %v", err)
	}
	if len(leftReport.Functions) != 2 || len(rightReport.Functions) != 2 {
		t.Fatalf("init function counts = %d/%d, want two", len(leftReport.Functions), len(rightReport.Functions))
	}
	if leftReport.Functions[0].ID == leftReport.Functions[1].ID {
		t.Fatalf("duplicate init IDs collided: %+v", leftReport.Functions)
	}
	for index := range leftReport.Functions {
		if leftReport.Functions[index].ID != rightReport.Functions[index].ID {
			t.Fatalf(
				"init identity changed after line insertion: %q/%q",
				leftReport.Functions[index].ID,
				rightReport.Functions[index].ID,
			)
		}
		if leftReport.Functions[index].Fingerprint != rightReport.Functions[index].Fingerprint {
			t.Fatalf("init fingerprint changed after line insertion")
		}
	}
}

func TestAnalyzeAnonymousFunctionIdentityFollowsContent(t *testing.T) {
	left := t.TempDir()
	right := t.TempDir()
	writeTestFile(t, left, "sample.go", `package sample

func Outer() {
	_ = func() int { return 1 }
	_ = func() int { return 2 }
}
`)
	writeTestFile(t, right, "sample.go", `package sample

func Outer() {
	_ = func() int { return 2 }
	_ = func() int { return 1 }
}
`)

	leftReport, err := Analyze(t.Context(), left, DefaultConfig())
	if err != nil {
		t.Fatalf("left Analyze() error = %v", err)
	}
	rightReport, err := Analyze(t.Context(), right, DefaultConfig())
	if err != nil {
		t.Fatalf("right Analyze() error = %v", err)
	}
	leftIDs := make(map[string]string, len(leftReport.Functions))
	for _, function := range leftReport.Functions {
		if function.Name == "Outer" {
			continue
		}

		leftIDs[function.Fingerprint] = function.ID
	}
	for _, function := range rightReport.Functions {
		if function.Name == "Outer" {
			continue
		}

		leftID, ok := leftIDs[function.Fingerprint]
		if !ok || leftID != function.ID {
			t.Fatalf(
				"anonymous identity followed ordinal: left=%q right=%q",
				leftIDs[function.Fingerprint],
				function.ID,
			)
		}
		if function.IdentityAmbiguous {
			t.Fatalf("distinct anonymous function marked ambiguous: %+v", function)
		}
	}
}

func TestSourceManifestDigestIsPathStableAndOrderIndependent(t *testing.T) {
	left := sourceManifestDigest([]*sourceFile{
		{path: "z.go", source: []byte("package z\n")},
		{path: "a.go", source: []byte("package a\n")},
	})
	right := sourceManifestDigest([]*sourceFile{
		{path: "a.go", source: []byte("package a\n")},
		{path: "z.go", source: []byte("package z\n")},
	})
	if left == "" || left != right {
		t.Fatalf("manifest digest = %q/%q, want stable digest", left, right)
	}
	changed := sourceManifestDigest([]*sourceFile{
		{path: "a.go", source: []byte("package changed\n")},
		{path: "z.go", source: []byte("package z\n")},
	})
	if changed == left {
		t.Fatalf("manifest digest did not change with source content: %q", left)
	}
}

func TestStructuralSignalsUseSyntacticShapes(t *testing.T) {
	source := []byte(`package sample

func Production(value int) int {
	if value < 0 {
		return 0
	}
	if value > 10 {
		if value > 100 {
			return 100
		}
		value = 10
	}
	result := struct{ Value int }{Value: value}
	return result.Value
}

func TestProduction(t *testing.T) {
	assert.Equal(t, 1, 1)
	require.NoError(t, nil)
	t.Errorf("failure")
	_ = len([]int{1})
	custom.Equal(1, 1)
}
`)
	fileSet := token.NewFileSet()
	parsed, err := parser.ParseFile(fileSet, "signals.go", source, parser.ParseComments)
	if err != nil {
		t.Fatalf("parse signals source: %v", err)
	}
	productionFile := &sourceFile{
		path:     "signals.go",
		source:   source,
		category: report.CategoryProduction,
		fileSet:  fileSet,
		astFile:  parsed,
	}
	nodes := collectFunctions(productionFile)
	productionSignals := structuralSignalsFor(productionFile, nodes[0])
	if productionSignals.flatGuards != 1 ||
		productionSignals.fieldMappings != 1 ||
		productionSignals.nestedBranches != 1 {
		t.Fatalf("production signals = %+v, want flat=1 field=1 nested=1", productionSignals)
	}

	testNode := nodes[1]
	testFile := *productionFile
	testFile.category = report.CategoryTest
	testSignals := structuralSignalsFor(&testFile, testNode)
	if testSignals.assertionLikeCalls != 3 {
		t.Fatalf("test assertion-like calls = %d, want 3", testSignals.assertionLikeCalls)
	}
}

func TestAnalyzeBranchFindingIsAReviewCandidate(t *testing.T) {
	root := t.TempDir()
	writeTestFile(t, root, "branches.go", `package sample

func Branches(value int) int {
	if value > 0 {
		return 1
	} else {
		return 1
	}
}
`)

	result, err := Analyze(t.Context(), root, DefaultConfig())
	if err != nil {
		t.Fatalf("Analyze() error = %v", err)
	}

	if len(result.Findings) != 1 || result.Findings[0].Rule != "duplicate-branches" {
		t.Fatalf("findings = %+v", result.Findings)
	}

	if result.Findings[0].Severity != "review" {
		t.Fatalf("finding severity = %q, want review", result.Findings[0].Severity)
	}
}

func TestAnalyzeCoverageClassificationAndSymlinks(t *testing.T) {
	root := t.TempDir()
	writeTestFile(t, root, "main.go", "package sample\nfunc Main() {}\n")
	writeTestFile(t, root, "main_test.go", "package sample\nfunc TestMain() {}\n")
	writeTestFile(t, root, "testdata/fixture.go", "package fixture\nfunc Fixture() {}\n")
	writeTestFile(t, root, "vendor/dependency.go", "package dependency\nfunc Dependency() {}\n")
	writeTestFile(
		t,
		root,
		"generated.go",
		"/* generated license */\n// Code generated by tool; DO NOT EDIT.\npackage sample\nfunc Generated() {}\n",
	)
	writeTestFile(t, root, "README.md", "not Go\n")
	if err := os.Symlink(filepath.Join(root, "main.go"), filepath.Join(root, "linked.go")); err != nil {
		t.Skipf("symlinks unavailable: %v", err)
	}

	result, err := Analyze(t.Context(), root, DefaultConfig())
	if err != nil {
		t.Fatalf("Analyze() error = %v", err)
	}

	if result.Coverage.Discovered != 7 || result.Coverage.Analyzed != 2 || result.Coverage.Excluded != 4 ||
		result.Coverage.Unsupported != 1 ||
		result.Coverage.Symlinks != 1 {
		t.Fatalf("coverage = %+v", result.Coverage)
	}

	statuses := map[string]report.File{}
	for _, file := range result.Files {
		statuses[file.Path] = file
	}

	for path, category := range map[string]report.Category{
		"generated.go":         report.CategoryGenerated,
		"vendor/dependency.go": report.CategoryVendor,
		"testdata/fixture.go":  report.CategoryTestdata,
	} {
		file, ok := statuses[path]
		if !ok || file.Status != report.FileExcluded || file.Category != category {
			t.Fatalf("%s = %+v", path, file)
		}
	}

	if statuses["linked.go"].Reason != "symlink not followed" {
		t.Fatalf("symlink = %+v", statuses["linked.go"])
	}

	if result.Functions[0].Path != "main.go" || result.Functions[1].Path != "main_test.go" {
		t.Fatalf("functions include excluded files: %+v", result.Functions)
	}
}

func TestAnalyzeParseFailureIsReportedOnce(t *testing.T) {
	root := t.TempDir()
	writeTestFile(t, root, "broken.go", "package broken\nfunc (")

	result, err := Analyze(t.Context(), root, DefaultConfig())
	if err == nil {
		t.Fatal("Analyze() error = nil, want parse failure")
	}

	var analysisErr *AnalysisError
	if !errors.As(err, &analysisErr) || len(analysisErr.Failures) != 1 {
		t.Fatalf("error = %T %v", err, err)
	}

	if result.Coverage.ParseErrors != 1 || len(result.Files) != 1 || result.Files[0].Status != report.FileParseError {
		t.Fatalf("report = %+v", result)
	}
}

func TestAnalyzeDeterministicAndExplicitDuplicationStatus(t *testing.T) {
	root := t.TempDir()
	writeTestFile(t, root, "sample.go", "package sample\nfunc Main() {}\n")
	cfg := DefaultConfig()
	first, firstErr := Analyze(t.Context(), root, cfg)
	second, secondErr := Analyze(t.Context(), root, cfg)
	if firstErr != nil || secondErr != nil {
		t.Fatalf("Analyze() errors = %v, %v", firstErr, secondErr)
	}

	firstJSON, _ := json.Marshal(first)
	secondJSON, _ := json.Marshal(second)
	if !bytes.Equal(firstJSON, secondJSON) {
		t.Fatalf("reports differ:\n%s\n%s", firstJSON, secondJSON)
	}

	if first.Duplication == nil || first.Duplication.Status != report.DuplicationNotRequested {
		t.Fatalf("duplication = %+v", first.Duplication)
	}

	if strings.Contains(string(firstJSON), root) {
		t.Fatalf("report leaked absolute root: %s", firstJSON)
	}

	cfg.Duplication.Requested = true
	cfg.Duplication.Tool = filepath.Join(root, "missing-cpd")
	requested, requestedErr := Analyze(t.Context(), root, cfg)
	if requestedErr == nil {
		t.Fatal("requested duplication error = nil, want unavailable tool")
	}

	if requested.Duplication == nil || requested.Duplication.Status != report.DuplicationError {
		t.Fatalf("requested duplication = %+v", requested.Duplication)
	}

	if !strings.Contains(requested.Duplication.Error, "unavailable") {
		t.Fatalf("requested duplication error = %q", requested.Duplication.Error)
	}
}

func TestAnalyzeNonGoInputIsExplicitlyUnsupported(t *testing.T) {
	root := t.TempDir()
	writeTestFile(t, root, "notes.txt", "notes\n")
	result, err := Analyze(t.Context(), root, DefaultConfig())
	if err != nil {
		t.Fatalf("Analyze() error = %v", err)
	}

	if result.Coverage.Analyzed != 0 || result.Coverage.Unsupported != 1 || len(result.Files) != 1 ||
		result.Files[0].Status != report.FileUnsupported {
		t.Fatalf("report = %+v", result)
	}
}

func TestAnalyzeDuplicationAdapterReportsNormalizedClones(t *testing.T) {
	tool, err := exec.LookPath("cpd")
	if err != nil {
		t.Skipf("cpd unavailable: %v", err)
	}

	versionOutput, err := exec.Command(tool, "--version").CombinedOutput()
	if err != nil || !strings.Contains(string(versionOutput), duplicationToolVersion) {
		t.Skipf("supported cpd %s unavailable: %s", duplicationToolVersion, versionOutput)
	}

	root := t.TempDir()
	duplicate := `	values := []int{1, 2, 3, 4, 5, 6, 7, 8}
	total := 0
	for _, value := range values {
		total += value
	}
	if total == 0 {
		return 0
	}
	return total
`
	writeTestFile(t, root, "one.go", "package sample\nfunc One() int {\n"+duplicate+"}\n")
	writeTestFile(t, root, "two.go", "package sample\nfunc Two() int {\n"+duplicate+"}\n")
	cfg := DefaultConfig()
	cfg.Duplication.Requested = true
	cfg.Duplication.MinTokens = 20
	cfg.Duplication.MinLines = 3
	result, err := Analyze(t.Context(), root, cfg)
	if err != nil {
		t.Fatalf("Analyze() error = %v", err)
	}

	if result.Duplication == nil || result.Duplication.Status != report.DuplicationMeasured {
		t.Fatalf("duplication = %+v", result.Duplication)
	}

	if len(result.Duplication.Clones) == 0 {
		t.Fatalf("duplication clones = 0, want a repeated block")
	}

	knownLocation := false
	for _, clone := range result.Duplication.Clones {
		for _, location := range clone.Locations {
			if location.Path == "one.go" && location.Start.Line == 2 && location.Start.Column == 9 &&
				location.End.Line == 12 &&
				location.End.Column == 1 {
				knownLocation = true
			}
		}
	}

	if !knownLocation {
		t.Fatalf("normalized clone location missing from %+v", result.Duplication.Clones)
	}

	for _, clone := range result.Duplication.Clones {
		if clone.Category != report.CategoryProduction || len(clone.Locations) != 2 {
			t.Fatalf("clone = %+v", clone)
		}

		for _, location := range clone.Locations {
			if strings.Contains(location.Path, "deburr-duplication") || filepath.IsAbs(location.Path) {
				t.Fatalf("clone leaked snapshot path: %+v", location)
			}
		}
	}

	singleRoot := t.TempDir()
	writeTestFile(
		t,
		singleRoot,
		"single.go",
		"package sample\nfunc One() int {\n"+duplicate+"}\nfunc Two() int {\n"+duplicate+"}\n",
	)
	single, err := Analyze(t.Context(), singleRoot, cfg)
	if err != nil {
		t.Fatalf("single-file Analyze() error = %v", err)
	}

	if single.Duplication == nil || single.Duplication.Status != report.DuplicationMeasured ||
		len(single.Duplication.Clones) == 0 {
		t.Fatalf("single-file duplication = %+v", single.Duplication)
	}
}

func TestNormalizeCPDClonesKeepsOccurrenceIdentityAndIgnoresLineMoves(t *testing.T) {
	leftSource := []byte(`package sample

func One() int {
	return 1
}

func Two() int {
	return 2
}
`)
	rightSource := []byte(`package sample


// an inserted line shifts both clone locations

func One() int {
	return 1
}

func Two() int {
	return 2
}
`)

	left, err := normalizeCPDClones([]cpdDuplicate{
		{
			FirstFile: cpdFile{
				Name: "one.go", StartLoc: cpdPoint{Line: 4, Column: 2}, EndLoc: cpdPoint{Line: 4, Column: 9},
			},
			SecondFile: cpdFile{
				Name: "two.go", StartLoc: cpdPoint{Line: 8, Column: 2}, EndLoc: cpdPoint{Line: 8, Column: 9},
			},
			Tokens: 4,
			Lines:  1,
		},
	}, report.CategoryProduction, "/tmp/left", map[string][]byte{
		"one.go": leftSource,
		"two.go": leftSource,
	})
	if err != nil {
		t.Fatalf("normalize left clones: %v", err)
	}

	right, err := normalizeCPDClones([]cpdDuplicate{
		{
			FirstFile: cpdFile{
				Name: "one.go", StartLoc: cpdPoint{Line: 7, Column: 2}, EndLoc: cpdPoint{Line: 7, Column: 9},
			},
			SecondFile: cpdFile{
				Name: "two.go", StartLoc: cpdPoint{Line: 11, Column: 2}, EndLoc: cpdPoint{Line: 11, Column: 9},
			},
			Tokens: 4,
			Lines:  1,
		},
	}, report.CategoryProduction, "/tmp/right", map[string][]byte{
		"one.go": rightSource,
		"two.go": rightSource,
	})
	if err != nil {
		t.Fatalf("normalize right clones: %v", err)
	}

	if len(left) != 1 || len(right) != 1 {
		t.Fatalf("clone counts = %d/%d, want one each", len(left), len(right))
	}
	if left[0].ID != right[0].ID || left[0].Fingerprint != right[0].Fingerprint {
		t.Fatalf("line move changed clone identity: left=%+v right=%+v", left[0], right[0])
	}
	if left[0].Locations[0].Start.Line == right[0].Locations[0].Start.Line {
		t.Fatalf("test did not shift clone location: left=%+v right=%+v", left[0], right[0])
	}
}

func TestNormalizeCPDClonesPreservesRepeatedPairsAndEndpointContent(t *testing.T) {
	source := []byte(`package sample

func One() int {
	return 1
}

func Two() int {
	return 2
}
`)
	duplicates := []cpdDuplicate{
		{
			FirstFile: cpdFile{
				Name: "one.go", StartLoc: cpdPoint{Line: 4, Column: 2}, EndLoc: cpdPoint{Line: 4, Column: 9},
			},
			SecondFile: cpdFile{
				Name: "two.go", StartLoc: cpdPoint{Line: 8, Column: 2}, EndLoc: cpdPoint{Line: 8, Column: 9},
			},
			Tokens: 4,
			Lines:  1,
		},
		{
			FirstFile: cpdFile{
				Name: "one.go", StartLoc: cpdPoint{Line: 4, Column: 2}, EndLoc: cpdPoint{Line: 4, Column: 9},
			},
			SecondFile: cpdFile{
				Name: "two.go", StartLoc: cpdPoint{Line: 8, Column: 2}, EndLoc: cpdPoint{Line: 8, Column: 9},
			},
			Tokens: 4,
			Lines:  1,
		},
	}

	clones, err := normalizeCPDClones(
		duplicates,
		report.CategoryProduction,
		"/tmp/snapshot",
		map[string][]byte{
			"one.go": source,
			"two.go": source,
		},
	)
	if err != nil {
		t.Fatalf("normalize clones: %v", err)
	}
	if len(clones) != 2 || clones[0].ID == clones[1].ID {
		t.Fatalf("repeated clone IDs = %+v, want two distinct occurrences", clones)
	}
	if clones[0].Fingerprint != clones[1].Fingerprint {
		t.Fatalf("repeated clone family fingerprints differ: %+v", clones)
	}
	if !clones[0].IdentityAmbiguous || !clones[1].IdentityAmbiguous {
		t.Fatalf("repeated clone occurrences not marked ambiguous: %+v", clones)
	}

	changed, err := normalizeCPDClones([]cpdDuplicate{
		{
			FirstFile: cpdFile{
				Name: "one.go", StartLoc: cpdPoint{Line: 4, Column: 2}, EndLoc: cpdPoint{Line: 4, Column: 9},
			},
			SecondFile: cpdFile{
				Name: "two.go", StartLoc: cpdPoint{Line: 8, Column: 2}, EndLoc: cpdPoint{Line: 8, Column: 9},
			},
			Tokens: 4,
			Lines:  1,
		},
	}, report.CategoryProduction, "/tmp/snapshot", map[string][]byte{
		"one.go": []byte(`package sample

func One() int {
	return 1
}
`),
		"two.go": []byte(`package sample

func One() int {
	return 1
}

func Two() int {
	return 9
}
`),
	})
	if err != nil {
		t.Fatalf("normalize changed clones: %v", err)
	}
	if len(changed) != 1 || clones[0].Fingerprint == changed[0].Fingerprint {
		t.Fatalf(
			"endpoint content change did not change family fingerprint: before=%+v after=%+v",
			clones[0],
			changed[0],
		)
	}
}

func TestNormalizeCPDClonesGroupsConnectedFamilies(t *testing.T) {
	source := []byte(`package sample

func A() int {
return 1
}

func B() int {
return 1
}

func C() int {
return 1
}

func X() int {
return 2
}

func Y() int {
return 2
}
`)
	line := func(value int) cpdPoint {
		return cpdPoint{Line: value, Column: 0}
	}
	duplicates := []cpdDuplicate{
		{
			FirstFile:  cpdFile{Name: "sample.go", StartLoc: line(4), EndLoc: cpdPoint{Line: 4, Column: 8}},
			SecondFile: cpdFile{Name: "sample.go", StartLoc: line(8), EndLoc: cpdPoint{Line: 8, Column: 8}},
		},
		{
			FirstFile:  cpdFile{Name: "sample.go", StartLoc: line(4), EndLoc: cpdPoint{Line: 4, Column: 8}},
			SecondFile: cpdFile{Name: "sample.go", StartLoc: line(12), EndLoc: cpdPoint{Line: 12, Column: 8}},
		},
		{
			FirstFile:  cpdFile{Name: "sample.go", StartLoc: line(8), EndLoc: cpdPoint{Line: 8, Column: 8}},
			SecondFile: cpdFile{Name: "sample.go", StartLoc: line(12), EndLoc: cpdPoint{Line: 12, Column: 8}},
		},
		{
			FirstFile:  cpdFile{Name: "sample.go", StartLoc: line(16), EndLoc: cpdPoint{Line: 16, Column: 8}},
			SecondFile: cpdFile{Name: "sample.go", StartLoc: line(20), EndLoc: cpdPoint{Line: 20, Column: 8}},
		},
	}

	clones, err := normalizeCPDClones(
		duplicates,
		report.CategoryProduction,
		"/tmp/snapshot",
		map[string][]byte{"sample.go": source},
	)
	if err != nil {
		t.Fatalf("normalize clones: %v", err)
	}
	if len(clones) != len(duplicates) {
		t.Fatalf("clone count = %d, want %d", len(clones), len(duplicates))
	}

	families := make(map[string][]report.Clone)
	for _, clone := range clones {
		families[clone.FamilyID] = append(families[clone.FamilyID], clone)
	}
	if len(families) != 2 {
		t.Fatalf("families = %+v, want connected and unrelated families", families)
	}
	for familyID, familyClones := range families {
		if len(familyClones) != 3 && len(familyClones) != 1 {
			t.Fatalf("family %q has %d pairs, want three or one: %+v", familyID, len(familyClones), families)
		}
	}
}

func TestAnalyzeDuplicationVersionMismatchIsIncomplete(t *testing.T) {
	if _, err := exec.LookPath("cpd"); err != nil {
		t.Skipf("cpd unavailable: %v", err)
	}

	root := t.TempDir()
	writeTestFile(t, root, "sample.go", "package sample\nfunc Main() {}\n")
	cfg := DefaultConfig()
	cfg.Duplication.Requested = true
	cfg.Duplication.Version = "0.0.0"
	result, err := Analyze(t.Context(), root, cfg)
	if err == nil {
		t.Fatal("Analyze() error = nil, want pinned version failure")
	}

	if result.Duplication == nil || result.Duplication.Status != report.DuplicationError ||
		!strings.Contains(result.Duplication.Error, "unsupported") {
		t.Fatalf("duplication = %+v", result.Duplication)
	}
}

func TestNormalizeCPDLocationUsesOneBasedInclusivePositions(t *testing.T) {
	location, err := normalizeCPDLocation(cpdFile{
		Name:     "production/sample.go",
		StartLoc: cpdPoint{Line: 1, Column: 0},
		EndLoc:   cpdPoint{Line: 2, Column: 0},
	}, report.CategoryProduction, "/tmp/snapshot", map[string][]byte{
		"sample.go": []byte("abc\ndef\n"),
	})
	if err != nil {
		t.Fatalf("normalizeCPDLocation() error = %v", err)
	}

	if location.Start.Line != 1 || location.Start.Column != 1 || location.End.Line != 1 || location.End.Column != 3 {
		t.Fatalf("location = %+v, want start 1:1 and inclusive end 1:3", location)
	}
}

func writeTestFile(t *testing.T, root, name, contents string) {
	t.Helper()
	path := filepath.Join(root, filepath.FromSlash(name))
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatal(err)
	}

	if err := os.WriteFile(path, []byte(contents), 0o644); err != nil {
		t.Fatal(err)
	}
}
