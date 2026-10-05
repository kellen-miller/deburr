package analysis

import (
	"io"
	"os"
	"testing"

	"github.com/kellen-miller/deburr/internal/report"
)

// VS Code bd64649be19f2da0f09c2ab84c066c6c764b83be supplies valid,
// real-world TypeScript. The upstream v0.55.1 grammar currently rejects seven
// files; this baseline records that limitation rather than treating the whole
// tree as successfully parsed. Remove entries when upstream fixes land.
func TestVSCodeTypeScriptIntegration(t *testing.T) {
	root := os.Getenv("DEBURR_VSCODE_SRC")
	if root == "" {
		t.Skip("set DEBURR_VSCODE_SRC to the pinned VS Code src checkout")
	}

	knownFailures := map[string]bool{
		"vs/base/common/observableInternal/map.ts":                            false,
		"vs/base/common/observableInternal/set.ts":                            false,
		"vs/code/electron-browser/workbench/workbench.ts":                     false,
		"vs/platform/browserView/electron-browser/preload-browserView.ts":     false,
		"vs/sessions/electron-browser/sessions.ts":                            false,
		"vs/workbench/contrib/terminalContrib/voice/browser/terminalVoice.ts": false,
		"vs/workbench/services/extensions/common/lazyPromise.ts":              false,
	}
	result, err := Analyze(t.Context(), root, DefaultConfig(), io.Discard)
	if err == nil || result.Coverage.ReadErrors != 0 || result.Coverage.ParseErrors != len(knownFailures) {
		t.Fatalf("expected known parser limitations only: %+v, %v", result.Coverage, err)
	}

	for _, file := range result.Files {
		if file.Status != report.FileParseError {
			continue
		}

		if _, known := knownFailures[file.Path]; !known {
			t.Errorf("new TypeScript parse failure: %s: %s", file.Path, file.Error)
		}

		knownFailures[file.Path] = true
	}

	for path, found := range knownFailures {
		if !found {
			t.Errorf("parser now accepts %s: remove it from the known-failure baseline", path)
		}
	}

	if result.Coverage.Analyzed != 10038 || len(result.Functions) != 269947 {
		t.Fatalf("pinned corpus coverage changed: analyzed=%d functions=%d", result.Coverage.Analyzed, len(result.Functions))
	}

	for _, function := range result.Functions {
		if !isTypeScriptPath(function.Path) || function.Start.Line < 1 || function.End.Line < function.Start.Line || function.SLOC < 0 || function.Cyclomatic < 1 || function.Fingerprint == "" {
			t.Fatalf("invalid measurement in corpus: %+v", function)
		}

		if knownFailures[function.Path] {
			t.Fatalf("rejected source produced partial function measurements: %+v", function)
		}
	}

	t.Logf("VS Code: %d files analyzed, %d functions, %d known upstream parse failures (not a clean parse)", result.Coverage.Analyzed, len(result.Functions), result.Coverage.ParseErrors)
}
