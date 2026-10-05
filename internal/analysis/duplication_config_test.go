package analysis

import (
	"encoding/json"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"testing"

	"github.com/kellen-miller/deburr/internal/report"
)

func TestDuplicationConfigurationScopesOriginalPaths(t *testing.T) {
	root := t.TempDir()
	for _, path := range []string{"src/one.ts", "src/two.ts", "src/skip.ts", "src/ignored.ts", "other/value.ts"} {
		writeTestFile(t, root, path, "export function value() { return 42; }\n")
	}

	writeTestFile(t, root, ".jscpd.json", `{"path":["src"],"ignore":["**/skip.ts"],"gitignore":true,"minTokens":20,"minLines":2,"threshold":5,"mode":"weak","reporters":["threshold"],"output":"ignored","ignoreIdentifiers":true}`)
	writeTestFile(t, root, ".gitignore", "src/ignored.ts\n")
	cfg := DefaultConfig().normalized()
	cfg.Duplication.Requested = true
	cfg = cfg.normalized()
	files := []*sourceFile{}
	for _, path := range []string{"src/one.ts", "src/two.ts", "src/skip.ts", "src/ignored.ts", "other/value.ts"} {
		files = append(files, &sourceFile{path: path, status: report.FileAnalyzed, category: report.CategoryProduction})
	}

	state := &analysisState{base: root, files: files, cfg: cfg, report: report.Report{Config: cfg.reportSummary()}}
	config, selected, err := configureDuplication(state)
	if err != nil || len(selected) != 2 || selected[0].path != "src/one.ts" || selected[1].path != "src/two.ts" {
		t.Fatalf("selected %+v, error %v", selected, err)
	}

	if config.MinTokens != 20 || config.MinLines != 2 || config.Threshold != "5" || config.Mode != "weak" || string(config.DetectorSettings) != `{"ignoreIdentifiers":true}` {
		t.Fatalf("effective settings %+v", config)
	}

	if state.report.Config.Duplication.GitignoreDigest == "" || state.report.Config.Duplication.ScopeRoot != "." {
		t.Fatalf("provenance %+v", state.report.Config.Duplication)
	}

	// A nested audit still resolves paths against the discovered config root.
	state.base = filepath.Join(root, "src")
	state.files = []*sourceFile{{path: "one.ts", status: report.FileAnalyzed, category: report.CategoryProduction}}
	_, selected, err = configureDuplication(state)
	if err != nil || len(selected) != 1 || state.report.Config.Duplication.ScopeRoot != ".." {
		t.Fatalf("nested audit %+v, %v", selected, err)
	}
}

func TestDuplicationGitignoreHierarchy(t *testing.T) {
	root := t.TempDir()
	writeTestFile(t, root, ".gitignore", "*.ts\n!keep.ts\nblocked/\nfile?.go\n")
	writeTestFile(t, root, "src/.gitignore", "!reinclude.ts\n/local.go\n*.tmp\n")
	writeTestFile(t, root, "blocked/.gitignore", "!value.ts\n")
	cache := make(map[string][]byte)
	for path, want := range map[string]bool{
		"value.ts": true, "keep.ts": false, "src/keep.ts": false,
		"src/reinclude.ts": false, "other/reinclude.ts": true,
		"blocked/value.ts": true, "src/local.go": true, "src/nested/local.go": false,
		"src/value.tmp": true, "other/value.tmp": false, "file1.go": true,
	} {
		got, err := duplicationGitignored(root, path, cache)
		if err != nil || got != want {
			t.Errorf("%s ignored=%v want=%v, %v", path, got, want, err)
		}
	}
}

func TestDuplicationConfigFailuresPreserveNativeReport(t *testing.T) {
	for _, config := range []string{
		`{"format":["javascript"]}`, `{"path":["missing"]}`, `{"ignore":["**/*.ts"]}`, `{"maxSize":1}`, `{"skipLocal":true}`,
		`{"threshold":-1}`, `{"threshold":101}`, `{"minTokens":0}`, `{"minLines":-2}`,
		`{"mode":"unknown"}`, `{"ignore":["[invalid"]}`, `{"path":[""]}`,
		`{"maxSize":"-9223372036854775807kb"}`, `{"maxSize":0}`, `{"semantic":true}`, `{"ignoreCase":"yes"}`, `{"maxLines":0}`, `[]`, `{`,
	} {
		t.Run(config, func(t *testing.T) {
			root := t.TempDir()
			writeTestFile(t, root, ".jscpd.json", config)
			writeTestFile(t, root, "source.ts", "const value = () => 42;\n")
			cfg := DefaultConfig()
			cfg.Duplication.Requested = true
			result, err := Analyze(t.Context(), root, cfg)
			if err == nil || result.Coverage.Analyzed != 1 || len(result.Functions) != 1 || result.Duplication.Status != report.DuplicationError {
				t.Fatalf("report %+v, error %v", result, err)
			}
		})
	}
}

func TestDuplicationExplicitConfigAndGitignoreOverride(t *testing.T) {
	root := t.TempDir()
	writeTestFile(t, root, ".jscpd.json", `{ "path":["nonexistent"] }`)
	writeTestFile(t, root, "custom.json", `{ "gitignore":false }`)
	writeTestFile(t, root, ".gitignore", "source.ts\n")
	writeTestFile(t, root, "source.ts", "const value = () => 42;\n")
	cfg := DefaultConfig()
	cfg.Duplication.Requested = true
	cfg.Duplication.Tool = writeVersionTool(t, "5.4.0")
	cfg.Duplication.ConfigPath = filepath.Join(root, "custom.json")
	result, err := Analyze(t.Context(), root, cfg)
	if err != nil || result.Duplication.Config.Gitignore || len(result.Duplication.Percentages) != 2 {
		t.Fatalf("report %+v, error %v", result.Duplication, err)
	}

	data, _ := json.Marshal(result)
	if strings.Contains(string(data), root) {
		t.Fatal("report leaked absolute config root")
	}

	cfg.Duplication.ConfigPath = filepath.Join(root, "missing.json")
	result, err = Analyze(t.Context(), root, cfg)
	if err == nil || result.Duplication.Status != report.DuplicationError {
		t.Fatal("missing explicit configuration silently ignored")
	}
}

func TestDuplicationRealConfigAndThresholdPolicy(t *testing.T) {
	tool, err := exec.LookPath("jscpd")
	if err != nil {
		t.Skip("jscpd unavailable")
	}

	if _, err := verifyDuplicationTool(t.Context(), DuplicationConfig{Tool: tool}, tool); err != nil {
		t.Skipf("major 5 unavailable: %v", err)
	}

	root := t.TempDir()
	source := "export function value(n: number) {\n  if (n > 1) {\n    console.log(n);\n  }\n  return n * 2;\n}\n"
	writeTestFile(t, root, "src/one.ts", source)
	writeTestFile(t, root, "src/two.ts", source)
	writeTestFile(t, root, "src/ignored.ts", source)
	writeTestFile(t, root, "outside/value.ts", source)
	writeTestFile(t, root, ".jscpd.json", `{"path":["src"],"ignore":["**/ignored.ts"],"minTokens":10,"minLines":1,"threshold":0,"reporters":["threshold"]}`)
	cfg := DefaultConfig()
	cfg.Duplication = DuplicationConfig{Requested: true, Tool: tool}
	advisory, err := Analyze(t.Context(), root, cfg)
	if err != nil || !advisory.Duplication.ThresholdExceeded || len(advisory.Duplication.Clones) == 0 {
		t.Fatalf("advisory %+v, error %v", advisory.Duplication, err)
	}

	for _, clone := range advisory.Duplication.Clones {
		for _, location := range clone.Locations {
			if location.Path != "src/one.ts" && location.Path != "src/two.ts" {
				t.Fatalf("excluded source cloned: %+v", location)
			}
		}
	}

	if advisory.Coverage.Analyzed != 4 {
		t.Fatal("duplication config narrowed native coverage")
	}

	cfg.Duplication.EnforceThreshold = true
	enforced, err := Analyze(t.Context(), root, cfg)
	if err == nil || enforced.Duplication.Status != report.DuplicationMeasured || len(enforced.Duplication.Clones) != len(advisory.Duplication.Clones) {
		t.Fatalf("threshold failed to preserve measured evidence: %+v, %v", enforced.Duplication, err)
	}

	writeTestFile(t, root, ".jscpd.json", `{"path":["src"],"ignore":["**/two.ts","**/ignored.ts"],"minTokens":10,"minLines":1,"threshold":0}`)
	clean, err := Analyze(t.Context(), root, cfg)
	if err != nil || clean.Duplication.ThresholdExceeded || len(clean.Duplication.Clones) != 0 {
		t.Fatalf("filtered threshold %+v, %v", clean.Duplication, err)
	}
}

func TestDuplicationFormatPatternAndSize(t *testing.T) {
	root := t.TempDir()
	writeTestFile(t, root, ".jscpd.json", `{"format":["typescript"],"pattern":"src/**/*.ts","maxSize":"1kb"}`)
	cfg := DefaultConfig()
	cfg.Duplication.Requested = true
	cfg = cfg.normalized()
	state := &analysisState{base: root, cfg: cfg, report: report.Report{Config: cfg.reportSummary()}, files: []*sourceFile{
		{path: "src/one.ts", bytes: 30, status: report.FileAnalyzed, category: report.CategoryProduction},
		{path: "src/large.ts", bytes: 1025, status: report.FileAnalyzed, category: report.CategoryProduction},
		{path: "src/go.go", bytes: 30, status: report.FileAnalyzed, category: report.CategoryProduction},
		{path: "elsewhere/one.ts", bytes: 30, status: report.FileAnalyzed, category: report.CategoryProduction},
	}}
	config, selected, err := configureDuplication(state)
	if err != nil || len(selected) != 1 || selected[0].path != "src/one.ts" || config.Formats != "typescript" || state.report.Config.Duplication.MaxFileBytes != 1024 {
		t.Fatalf("selected %+v, config %+v, error %v", selected, config, err)
	}
}

func TestDuplicationConfigOutsideSourceTree(t *testing.T) {
	root := t.TempDir()
	writeTestFile(t, root, "source.ts", "export const value = () => 42;\n")
	configPath := filepath.Join(root, ".github", "jscpd.json")
	cfg := DefaultConfig()
	cfg.Duplication = DuplicationConfig{Requested: true, Tool: writeVersionTool(t, "5.4.0"), ConfigPath: configPath, EnforceThreshold: true}
	for _, config := range []string{`{}`, `{"path":["../source.ts"]}`, `{"skipLocal":false}`} {
		writeTestFile(t, root, ".github/jscpd.json", config)
		result, err := Analyze(t.Context(), root, cfg)
		if err != nil || result.Duplication.Status != report.DuplicationMeasured || result.Duplication.Percentages[0].SelectedFiles != 1 {
			t.Fatalf("config %s: %+v, %v", config, result.Duplication, err)
		}
	}
}

func TestDuplicationNestedConfigIncludesAncestorGitignore(t *testing.T) {
	root := t.TempDir()
	writeTestFile(t, root, ".git", "gitdir: ignored\n")
	writeTestFile(t, root, ".gitignore", "ignored/\n")
	writeTestFile(t, root, "web/.jscpd.json", `{}`)
	cfg := DefaultConfig()
	cfg.Duplication.Requested = true
	cfg = cfg.normalized()
	state := &analysisState{base: filepath.Join(root, "web"), cfg: cfg, report: report.Report{Config: cfg.reportSummary()}, files: []*sourceFile{
		{path: "one.ts", status: report.FileAnalyzed, category: report.CategoryProduction},
		{path: "ignored/two.ts", status: report.FileAnalyzed, category: report.CategoryProduction},
	}}
	_, selected, err := configureDuplication(state)
	if err != nil || len(selected) != 1 || selected[0].path != "one.ts" {
		t.Fatalf("ancestor rules not applied: %+v, %v", selected, err)
	}

	before := state.report.Config.Duplication.GitignoreDigest
	writeTestFile(t, root, ".gitignore", "ignored/\n# changed policy source\n")
	_, _, err = configureDuplication(state)
	if err != nil || before == state.report.Config.Duplication.GitignoreDigest {
		t.Fatalf("ancestor ignore missing from identity: %v", err)
	}
}

func TestDuplicationRealLargeSourcesAndDetectorFilters(t *testing.T) {
	tool, err := exec.LookPath("jscpd")
	if err != nil {
		t.Skip("jscpd unavailable")
	}

	if _, err := verifyDuplicationTool(t.Context(), DuplicationConfig{Tool: tool}, tool); err != nil {
		t.Skipf("major 5 unavailable: %v", err)
	}

	root := t.TempDir()
	var source strings.Builder
	source.WriteString("export function value(n: number) {\n")
	for index := 0; index < 1500; index++ {
		source.WriteString("n += " + strconv.Itoa(index) + ";\n")
	}

	source.WriteString("return n;\n}\n")
	writeTestFile(t, root, "one.ts", source.String())
	writeTestFile(t, root, "two.ts", source.String())
	writeTestFile(t, root, ".jscpd.json", `{"minTokens":10,"minLines":1}`)
	cfg := DefaultConfig()
	cfg.Duplication = DuplicationConfig{Requested: true, Tool: tool}
	result, err := Analyze(t.Context(), root, cfg)
	if err != nil || len(result.Duplication.Clones) == 0 || result.Duplication.Percentages[0].Files != 2 || result.Duplication.Percentages[0].SelectedFiles != 2 {
		t.Fatalf("large sources were skipped: %+v, %v", result.Duplication, err)
	}

	writeTestFile(t, root, "small.ts", "export function small() {\nreturn 1;\n}\n")
	writeTestFile(t, root, ".jscpd.json", `{"minTokens":10,"minLines":1,"maxLines":1000}`)
	result, err = Analyze(t.Context(), root, cfg)
	if err != nil || result.Duplication.Percentages[0].Files != 1 || result.Duplication.Percentages[0].SelectedFiles != 3 {
		t.Fatalf("filtered source count is inaccurate: %+v, %v", result.Duplication, err)
	}

	writeTestFile(t, root, ".jscpd.json", `{"minTokens":10,"minLines":1,"maxLines":1000,"ignore":["small.ts"]}`)
	cfg.Duplication.EnforceThreshold = true
	result, err = Analyze(t.Context(), root, cfg)
	if err == nil || result.Duplication.Status != report.DuplicationError || !strings.Contains(err.Error(), "source count") {
		t.Fatalf("empty detector measurement passed: %+v, %v", result.Duplication, err)
	}
}
