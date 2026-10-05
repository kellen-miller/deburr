package analysis

import (
	"bytes"
	"encoding/json"
	"io"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"github.com/kellen-miller/deburr/internal/report"
)

func TestTypeScriptMetricsAndFindings(t *testing.T) {
	root := t.TempDir()
	writeTestFile(t, root, "source.ts", `// A comment.
export function check(value: number): boolean {
  const nested = (n: number) => {
    while (n > 0) { n--; }
    return n;
  };
  if (value > 0 && value < 10) { return true; } else { return false; }
}
class Service {
  run(value: number) {
    if (value > 0) { console.log(value); } else { console.log(value); }
  }
}
`)
	result, err := Analyze(t.Context(), root, DefaultConfig(), io.Discard)
	if err != nil {
		t.Fatal(err)
	}

	if len(result.Functions) != 3 || len(result.Findings) != 1 {
		t.Fatalf("functions %+v, findings %+v", result.Functions, result.Findings)
	}

	byName := make(map[string]report.Function)
	for _, function := range result.Functions {
		byName[function.Name] = function
	}

	if byName["check"].Cyclomatic != 3 || byName["check.nested"].Cyclomatic != 2 || byName["Service.run"].Cyclomatic != 2 {
		t.Fatalf("metrics = %+v", byName)
	}

	if byName["check"].MaxNesting != 1 || byName["check.nested"].MaxNesting != 1 || result.Files[0].CodeLines != 12 {
		t.Fatalf("nesting or code lines: %+v, %+v", byName, result.Files)
	}

	second, err := Analyze(t.Context(), root, DefaultConfig(), io.Discard)
	firstJSON, _ := json.Marshal(result)
	secondJSON, _ := json.Marshal(second)
	if err != nil || !bytes.Equal(firstJSON, secondJSON) {
		t.Fatal("TypeScript report is not deterministic")
	}
}

func TestTypeScriptCoverageAndSyntax(t *testing.T) {
	root := t.TempDir()
	for path, source := range map[string]string{
		"component.tsx":             "export const View = () => <div title=\"hello\">Hello</div>;\n",
		"module.mts":                "export function value<T>(v: T): T { return v; }\n",
		"module.cts":                "export const value = () => 42;\n",
		"types.d.ts":                "declare function value(v: string): number;\n",
		"source.test.ts":            "const test = () => true;\n",
		"source.spec.tsx":           "const test = () => <div />;\n",
		"__tests__/suite.ts":        "const test = () => true;\n",
		"testdata/fixture.ts":       "const ignored = () => true;\n",
		"node_modules/pkg/index.ts": "invalid syntax {{\n",
		"broken.ts":                 "export function broken( {\n",
	} {
		writeTestFile(t, root, path, source)
	}

	result, err := Analyze(t.Context(), root, DefaultConfig(), io.Discard)
	if err == nil || result.Coverage.ParseErrors != 1 || result.Coverage.Analyzed != 7 || result.Coverage.Excluded != 1 || result.Coverage.ExcludedDirectories != 1 {
		t.Fatalf("coverage %+v, error %v", result.Coverage, err)
	}

	if result.Metrics.Test.Functions != 3 || result.Metrics.Production.Functions != 3 {
		t.Fatalf("metrics %+v", result.Metrics)
	}

	cfg := DefaultConfig()
	cfg.ExcludeTests = true
	result, _ = Analyze(t.Context(), root, cfg, io.Discard)
	if result.Metrics.Test.Functions != 0 || result.Coverage.Excluded != 4 {
		t.Fatalf("exclude tests %+v", result)
	}
}

func TestTypeScriptFindingIdentitySurvivesComments(t *testing.T) {
	root := t.TempDir()
	source := "export function check(n: number) { if (n > 1) { return true; } else { return false; } }\n"
	writeTestFile(t, root, "source.ts", source)
	before, err := Analyze(t.Context(), root, DefaultConfig(), io.Discard)
	if err != nil {
		t.Fatal(err)
	}

	writeTestFile(t, root, "source.ts", "// shifted\n\n"+source)
	after, err := Analyze(t.Context(), root, DefaultConfig(), io.Discard)
	if err != nil || len(before.Findings) != 1 || len(after.Findings) != 1 || before.Findings[0].ID != after.Findings[0].ID || before.Functions[0].Fingerprint != after.Functions[0].Fingerprint {
		t.Fatalf("unstable identity: %+v -> %+v, %v", before.Findings, after.Findings, err)
	}
}

func TestTypeScriptDuplicationRealTools(t *testing.T) {
	for _, tool := range []string{"cpd", "jscpd"} {
		t.Run(tool, func(t *testing.T) {
			path, err := exec.LookPath(tool)
			if err != nil {
				t.Skipf("%s unavailable", tool)
			}

			version, err := verifyDuplicationTool(t.Context(), DuplicationConfig{Tool: tool}, path)
			if err != nil {
				t.Skipf("%s major 5 unavailable: %v", tool, err)
			}

			root := t.TempDir()
			source := `export function sum(values: number[]) {
  let total = 0;
  for (const value of values) {
    if (value > 0) {
      total += value;
    }
  }
  console.log(total);
  return total;
}
`
			for _, extension := range []string{".ts", ".tsx", ".mts", ".cts"} {
				formatSource := source
				if extension == ".tsx" {
					formatSource = strings.ReplaceAll(source, "return total;", "return <div>{total}</div>;")
				}

				writeTestFile(t, root, "one"+extension, formatSource)
				writeTestFile(t, root, "two"+extension, formatSource)
			}
			writeTestFile(t, root, "separate.test.ts", source)
			cfg := DefaultConfig()
			cfg.Duplication = DuplicationConfig{Requested: true, Tool: path, MinTokens: 20, MinLines: 3}
			result, err := Analyze(t.Context(), root, cfg, io.Discard)
			if err != nil || len(result.Duplication.Clones) == 0 || result.Duplication.Config.Version != version {
				t.Fatalf("clones %+v, error %v", result.Duplication, err)
			}

			formats := make(map[string]bool)
			for _, clone := range result.Duplication.Clones {
				for _, location := range clone.Locations {
					formats[filepath.Ext(location.Path)] = true
					if strings.Contains(location.Path, "test") || location.Start.Line < 1 || location.Start.Column < 1 {
						t.Fatalf("clone scope or position %+v", clone)
					}
				}
			}
			for _, extension := range []string{".ts", ".tsx", ".mts", ".cts"} {
				if !formats[extension] {
					t.Fatalf("no measured clones for %s", extension)
				}
			}
		})
	}
}

func TestTypeScriptLiteralsAndGeneratedCoverage(t *testing.T) {
	root := t.TempDir()
	source := "export const pattern = () => {\n// comment\nconst regex = /a{2}\\/b/;\nconst message = `first\n  second`;\nreturn message;\n};\n"
	writeTestFile(t, root, "source.ts", source)
	writeTestFile(t, root, "generated.ts", "// Code generated by example. DO NOT EDIT.\nexport const generated = () => true;\n")
	before, err := Analyze(t.Context(), root, DefaultConfig(), io.Discard)
	if err != nil || len(before.Functions) != 1 || before.Coverage.Excluded != 1 || before.Functions[0].SLOC != 6 {
		t.Fatalf("report %+v, error %v", before, err)
	}

	writeTestFile(t, root, "source.ts", strings.ReplaceAll(source, "  second", "second"))
	after, err := Analyze(t.Context(), root, DefaultConfig(), io.Discard)
	if err != nil || before.Functions[0].Fingerprint == after.Functions[0].Fingerprint {
		t.Fatal("template whitespace change lost from fingerprint")
	}
}

func TestTypeScriptCloneIdentity(t *testing.T) {
	source := []byte("export function value() {\n  return `hello world`;\n}\n")
	shifted := append([]byte("// comment\n"), source...)
	before, err := normalizeTestCPDClones(t, []cpdDuplicate{{
		FirstFile:  cpdFile{Name: "one.ts", StartLoc: cpdPoint{Line: 2, Column: 2}, EndLoc: cpdPoint{Line: 2, Column: 23}},
		SecondFile: cpdFile{Name: "two.ts", StartLoc: cpdPoint{Line: 2, Column: 2}, EndLoc: cpdPoint{Line: 2, Column: 23}},
	}}, report.CategoryProduction, "", map[string][]byte{"one.ts": source, "two.ts": source})
	if err != nil {
		t.Fatal(err)
	}

	after, err := normalizeTestCPDClones(t, []cpdDuplicate{{
		FirstFile:  cpdFile{Name: "one.ts", StartLoc: cpdPoint{Line: 3, Column: 2}, EndLoc: cpdPoint{Line: 3, Column: 23}},
		SecondFile: cpdFile{Name: "two.ts", StartLoc: cpdPoint{Line: 3, Column: 2}, EndLoc: cpdPoint{Line: 3, Column: 23}},
	}}, report.CategoryProduction, "", map[string][]byte{"one.ts": shifted, "two.ts": shifted})
	if err != nil || before[0].ID != after[0].ID || before[0].Fingerprint != after[0].Fingerprint {
		t.Fatalf("unstable clone identity: %+v -> %+v, %v", before, after, err)
	}
}

func TestTypeScriptBooleanGuardsIgnoreComments(t *testing.T) {
	root := t.TempDir()
	for _, source := range []string{
		"function value(n: number) { if (n > 0) { return true; } return false; }",
		"function value(n: number) { if (n > 0) { /* comment */ return true; } /* gap */ return false; }",
		"function value(n: number) { if (n > 0) { /* comment */ return true; } else { /* comment */ return false; } }",
	} {
		writeTestFile(t, root, "source.ts", source)
		result, err := Analyze(t.Context(), root, DefaultConfig(), io.Discard)
		if err != nil || len(result.Findings) != 1 || result.Findings[0].Rule != "redundant-boolean-return" {
			t.Fatalf("findings %+v, error %v", result.Findings, err)
		}
	}
}

func normalizeTestCPDClones(t *testing.T, duplicates []cpdDuplicate, category report.Category, root string, selected map[string][]byte) ([]report.Clone, error) {
	t.Helper()
	state := analysisState{}
	files := make([]*sourceFile, 0, len(selected))
	for path, source := range selected {
		file := &sourceFile{path: path, source: source, category: category, status: report.FileAnalyzed}
		state.parseFile(file)
		if file.status != report.FileAnalyzed {
			t.Fatalf("parse clone fixture %s: %s", path, file.err)
		}

		files = append(files, file)
	}

	return normalizeCPDClones(duplicates, category, root, selected, buildCloneSourceScopes(files))
}

func TestTypeScriptNestedControls(t *testing.T) {
	root := t.TempDir()
	writeTestFile(t, root, "nested.ts", `function nested(n: number) {
  if (n > 0) {
    for (let i = 0; i < n; i++) {
      while (n > 1) { n--; }
      switch (n) { case 1: break; }
    }
  }
}`)
	result, err := Analyze(t.Context(), root, DefaultConfig(), io.Discard)
	if err != nil {
		t.Fatal(err)
	}

	function := result.Functions[0]
	if function.StructuralSignals.NestedBranches != 3 || function.MaxNesting != 3 {
		t.Fatalf("nested control metrics: %+v", function)
	}
}

func TestTypeScriptClassFunctionOwners(t *testing.T) {
	root := t.TempDir()
	writeTestFile(t, root, "owners.ts", `abstract class A { run() { const nested = () => 1; return nested(); } }
abstract class B { run() { return 2; } }
function factory() {
  class Inner { run() { return 3; } }
  return Inner;
}
function run() { return 4; }
`)
	result, err := Analyze(t.Context(), root, DefaultConfig(), io.Discard)
	if err != nil {
		t.Fatal(err)
	}

	names := make(map[string]bool)
	for _, function := range result.Functions {
		if function.IdentityAmbiguous {
			t.Fatalf("class owner lost: %+v", function)
		}

		names[function.Name] = true
	}

	for _, name := range []string{"A.run", "A.run.nested", "B.run", "factory", "factory.Inner.run", "run"} {
		if !names[name] {
			t.Fatalf("missing %s in %+v", name, names)
		}
	}
}

func TestTypeScriptSharedParseMetadata(t *testing.T) {
	source := []byte("function value() { return 1; }\n")
	file := &sourceFile{path: "source.ts", source: source, status: report.FileAnalyzed}
	state := analysisState{}
	state.parseFile(file)
	if len(state.failures) != 0 || file.codeLines != 1 || len(file.tsFunctions) != 1 || len(file.tsTokens) == 0 {
		t.Fatalf("parsed metadata: %+v, failures: %+v", file, state.failures)
	}

	scope := buildCloneSourceScopes([]*sourceFile{file})[file.path]
	endpoint, err := normalizeCloneEndpoint(report.Location{
		Path: file.path, Start: report.Position{Line: 1, Column: 20}, End: report.Position{Line: 1, Column: 28},
	}, scope)
	if err != nil || endpoint.scope != state.functions[0].ID {
		t.Fatalf("clone endpoint should belong to parsed function: %+v, %v", endpoint, err)
	}

	broken := &sourceFile{path: "broken.ts", source: []byte("function broken( {"), status: report.FileAnalyzed}
	state.parseFile(broken)
	if broken.status != report.FileParseError || len(state.failures) != 1 || len(broken.tsTokens) != 0 || len(broken.lineCode) != 0 {
		t.Fatalf("failed parse must not yield measurements: %+v", broken)
	}
}

func TestTypeScriptExcludedCoverageCounts(t *testing.T) {
	root := t.TempDir()
	writeTestFile(t, root, "valid.test.ts", "// comment\nfunction value() {\n  return 1;\n}\n")
	writeTestFile(t, root, "invalid.test.ts", "function broken( {\n")
	cfg := DefaultConfig()
	cfg.ExcludeTests = true
	result, err := Analyze(t.Context(), root, cfg, io.Discard)
	if err != nil || len(result.Functions) != 0 || result.Coverage.Excluded != 2 || result.Coverage.ParseErrors != 0 {
		t.Fatalf("exclusion must own status: %+v, %v", result, err)
	}

	for _, file := range result.Files {
		if file.Path == "valid.test.ts" && file.CodeLines != 3 {
			t.Fatalf("valid excluded source lost code-line counts: %+v", file)
		}
	}
}
