package analysis

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/kellen-miller/deburr/internal/report"
)

// Re-execute the test binary as a portable caller-provided detector.
func TestMain(m *testing.M) {
	if version := os.Getenv("DEBURR_TEST_TOOL_VERSION"); version != "" && len(os.Args) > 1 && strings.HasPrefix(os.Args[1], "--") {
		if os.Args[1] == "--version" {
			fmt.Println("jscpd " + version)
			os.Exit(0)
		}

		for index, arg := range os.Args {
			if arg == "--output" && index+1 < len(os.Args) {
				err := os.WriteFile(filepath.Join(os.Args[index+1], "report.json"), []byte(`{"duplicates":[],"statistics":{"total":{"percentage":0,"sources":1}}}`), 0o600)
				if err != nil {
					os.Exit(2)
				}

				os.Exit(0)
			}
		}

		os.Exit(2)
	}

	os.Exit(m.Run())
}

func writeVersionTool(t *testing.T, version string) string {
	t.Helper()
	t.Setenv("DEBURR_TEST_TOOL_VERSION", version)
	path, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}

	return path
}

func TestDuplicationAcceptsMajorAndRecordsVersion(t *testing.T) {
	for _, version := range []string{"5.3.0", "5.4.0", "5.99.12", "4.9.0", "6.0.0", "unknown"} {
		t.Run(version, func(t *testing.T) {
			root := t.TempDir()
			writeTestFile(t, root, "source.ts", "export const value = () => 42;\n")
			cfg := DefaultConfig()
			cfg.Duplication.Requested = true
			cfg.Duplication.Tool = writeVersionTool(t, version)
			result, err := Analyze(t.Context(), root, cfg, io.Discard)
			if strings.HasPrefix(version, "5.") {
				if err != nil || result.Duplication.Config.Version != version || result.Config.Duplication.Version != version {
					t.Fatalf("version provenance = %+v, error %v", result.Duplication, err)
				}

				data, marshalErr := json.Marshal(result)
				if marshalErr != nil || strings.Contains(string(data), root) {
					t.Fatalf("report leaked root or failed: %s, %v", data, marshalErr)
				}
			} else if err == nil {
				t.Fatal("unsupported version accepted")
			}
		})
	}
}

func TestDuplicationProgressNamesDetectorAndCategory(t *testing.T) {
	root := t.TempDir()
	writeTestFile(t, root, "source.ts", "export function value() { return 42; }\n")
	cfg := DefaultConfig()
	cfg.Duplication = DuplicationConfig{Requested: true, Tool: writeVersionTool(t, "5.4.0")}
	var progress bytes.Buffer
	result, err := Analyze(t.Context(), root, cfg, &progress)
	if err != nil || result.Duplication.Status != report.DuplicationMeasured {
		t.Fatalf("duplication failed: %+v, %v", result.Duplication, err)
	}

	previous := -1
	for _, message := range []string{"loading duplication config", "checking duplication detector", "5.4.0; 1 files selected", "snapshotting 1 production files", "for production duplication", "production duplication finished: 0.00%; 1 of 1 files analyzed"} {
		position := strings.Index(progress.String(), message)
		if position <= previous {
			t.Fatalf("missing/out-of-order %q: %s", message, progress.String())
		}

		previous = position
	}
}
