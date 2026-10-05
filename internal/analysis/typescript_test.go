package analysis

import (
	"bytes"
	"encoding/json"
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
	result, err := Analyze(t.Context(), root, DefaultConfig())
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

	second, err := Analyze(t.Context(), root, DefaultConfig())
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

	result, err := Analyze(t.Context(), root, DefaultConfig())
	if err == nil || result.Coverage.ParseErrors != 1 || result.Coverage.Analyzed != 7 || result.Coverage.Excluded != 1 || result.Coverage.ExcludedDirectories != 1 {
		t.Fatalf("coverage %+v, error %v", result.Coverage, err)
	}

	if result.Metrics.Test.Functions != 3 || result.Metrics.Production.Functions != 3 {
		t.Fatalf("metrics %+v", result.Metrics)
	}

	cfg := DefaultConfig()
	cfg.ExcludeTests = true
	result, _ = Analyze(t.Context(), root, cfg)
	if result.Metrics.Test.Functions != 0 || result.Coverage.Excluded != 4 {
		t.Fatalf("exclude tests %+v", result)
	}
}

func TestTypeScriptFindingIdentitySurvivesComments(t *testing.T) {
	root := t.TempDir()
	source := "export function check(n: number) { if (n > 1) { return true; } else { return false; } }\n"
	writeTestFile(t, root, "source.ts", source)
	before, err := Analyze(t.Context(), root, DefaultConfig())
	if err != nil {
		t.Fatal(err)
	}

	writeTestFile(t, root, "source.ts", "// shifted\n\n"+source)
	after, err := Analyze(t.Context(), root, DefaultConfig())
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
			result, err := Analyze(t.Context(), root, cfg)
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
	before, err := Analyze(t.Context(), root, DefaultConfig())
	if err != nil || len(before.Functions) != 1 || before.Coverage.Excluded != 1 || before.Functions[0].SLOC != 6 {
		t.Fatalf("report %+v, error %v", before, err)
	}

	writeTestFile(t, root, "source.ts", strings.ReplaceAll(source, "  second", "second"))
	after, err := Analyze(t.Context(), root, DefaultConfig())
	if err != nil || before.Functions[0].Fingerprint == after.Functions[0].Fingerprint {
		t.Fatal("template whitespace change lost from fingerprint")
	}
}

func TestTypeScriptCloneIdentity(t *testing.T) {
	source := []byte("export function value() {\n  return `hello world`;\n}\n")
	shifted := append([]byte("// comment\n"), source...)
	before, err := normalizeCPDClones([]cpdDuplicate{{
		FirstFile:  cpdFile{Name: "one.ts", StartLoc: cpdPoint{Line: 2, Column: 2}, EndLoc: cpdPoint{Line: 2, Column: 23}},
		SecondFile: cpdFile{Name: "two.ts", StartLoc: cpdPoint{Line: 2, Column: 2}, EndLoc: cpdPoint{Line: 2, Column: 23}},
	}}, report.CategoryProduction, "", map[string][]byte{"one.ts": source, "two.ts": source})
	if err != nil {
		t.Fatal(err)
	}

	after, err := normalizeCPDClones([]cpdDuplicate{{
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
		result, err := Analyze(t.Context(), root, DefaultConfig())
		if err != nil || len(result.Findings) != 1 || result.Findings[0].Rule != "redundant-boolean-return" {
			t.Fatalf("findings %+v, error %v", result.Findings, err)
		}
	}
}
