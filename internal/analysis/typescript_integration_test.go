package analysis

import (
	"io"
	"os"
	"testing"
)

// VS Code bd64649be19f2da0f09c2ab84c066c6c764b83be supplies valid,
// real-world TypeScript. Every TypeScript source must parse successfully.
func TestVSCodeTypeScriptIntegration(t *testing.T) {
	root := os.Getenv("DEBURR_VSCODE_SRC")
	if root == "" {
		t.Skip("set DEBURR_VSCODE_SRC to the pinned VS Code src checkout")
	}

	result, err := Analyze(t.Context(), root, DefaultConfig(), io.Discard)
	if err != nil || result.Coverage.ReadErrors != 0 || result.Coverage.ParseErrors != 0 {
		t.Fatalf("expected complete TypeScript coverage: %+v, %v", result.Coverage, err)
	}

	if result.Coverage.Analyzed != 10045 || len(result.Functions) != 270201 {
		t.Fatalf(
			"pinned corpus coverage changed: analyzed=%d functions=%d",
			result.Coverage.Analyzed,
			len(result.Functions),
		)
	}

	for _, function := range result.Functions {
		if !isTypeScriptPath(function.Path) || function.Start.Line < 1 || function.End.Line < function.Start.Line ||
			function.SLOC < 0 ||
			function.Cyclomatic < 1 ||
			function.Fingerprint == "" {
			t.Fatalf("invalid measurement in corpus: %+v", function)
		}
	}

	t.Logf(
		"VS Code: %d files analyzed, %d functions, %d parse failures",
		result.Coverage.Analyzed,
		len(result.Functions),
		result.Coverage.ParseErrors,
	)
}
