package cli

import (
	"strings"
	"testing"

	"github.com/kellen-miller/deburr/internal/render"
)

func TestParseOptionsAcceptsFlagsAroundPath(t *testing.T) {
	options, positionals, err := parseOptions([]string{
		"--format=json",
		"--exclude-tests",
		"backend",
		"--max-file-bytes",
		"123",
		"--include-vendor",
	}, true)
	if err != nil {
		t.Fatal(err)
	}

	if len(positionals) != 1 || positionals[0] != "backend" {
		t.Fatalf("positionals = %#v", positionals)
	}

	if options.format != render.FormatJSON || !options.config.ExcludeTests || !options.config.IncludeVendor ||
		options.config.MaxFileBytes != 123 {
		t.Fatalf("options = %+v", options)
	}
}

func TestParseOptionsStopsAtSeparator(t *testing.T) {
	options, positionals, err := parseOptions([]string{"--format", "json", "--", "--literal"}, false)
	if err != nil {
		t.Fatal(err)
	}

	if options.format != render.FormatJSON || len(positionals) != 1 || positionals[0] != "--literal" {
		t.Fatalf("options=%+v positionals=%#v", options, positionals)
	}
}

func TestParseOptionsRejectsMalformedFlags(t *testing.T) {
	tests := []struct {
		name  string
		args  []string
		audit bool
		want  string
	}{
		{
			name:  "boolean value",
			args:  []string{"--exclude-tests=false", "root"},
			audit: true,
			want:  "does not take a value",
		},
		{name: "missing format", args: []string{"--format"}, audit: true, want: "requires a value"},
		{name: "unknown flag", args: []string{"--unknown", "root"}, audit: true, want: "unknown flag"},
		{
			name:  "compare audit flag",
			args:  []string{"--duplicates", "before", "after"},
			audit: false,
			want:  "only valid for audit",
		},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			_, _, err := parseOptions(test.args, test.audit)
			if err == nil || !strings.Contains(err.Error(), test.want) {
				t.Fatalf("error = %v, want %q", err, test.want)
			}
		})
	}
}

func TestParseOptionsAcceptsExplicitDuplicationTool(t *testing.T) {
	options, positionals, err := parseOptions(
		[]string{"--duplicates", "--cpd", "/tools/cpd", "repository"},
		true,
	)
	if err != nil {
		t.Fatal(err)
	}

	if len(positionals) != 1 || positionals[0] != "repository" {
		t.Fatalf("positionals = %#v", positionals)
	}

	if !options.config.Duplication.Requested || options.config.Duplication.Tool != "/tools/cpd" {
		t.Fatalf("duplication = %+v", options.config.Duplication)
	}
}

func TestJSCPDOption(t *testing.T) {
	options, paths, err := parseOptions([]string{"repo", "--jscpd", "/tools/jscpd"}, true)
	if err != nil || len(paths) != 1 || !options.config.Duplication.Requested || options.config.Duplication.Tool != "/tools/jscpd" {
		t.Fatalf("options %+v, paths %v, error %v", options, paths, err)
	}

	for _, args := range [][]string{
		{"--jscpd", ""},
		{"--cpd", "cpd", "--jscpd", "jscpd"},
	} {
		if _, _, err := parseOptions(args, true); err == nil {
			t.Fatalf("accepted invalid args %v", args)
		}
	}

	if _, _, err := parseOptions([]string{"--jscpd", "jscpd"}, false); err == nil {
		t.Fatal("accepted jscpd outside audit")
	}
}

func TestDuplicationConfigurationOptions(t *testing.T) {
	options, paths, err := parseOptions([]string{"repo", "--duplicates-config", "config.json", "--enforce-duplicates-threshold"}, true)
	if err != nil || len(paths) != 1 || !options.config.Duplication.Requested || !options.config.Duplication.EnforceThreshold || options.config.Duplication.ConfigPath != "config.json" {
		t.Fatalf("options %+v, paths %v, error %v", options, paths, err)
	}

	for _, args := range [][]string{
		{"--duplicates-config", ""},
		{"--enforce-duplicates-threshold=false"},
	} {
		if _, _, err := parseOptions(args, true); err == nil {
			t.Fatalf("accepted invalid args %v", args)
		}
	}

	if _, _, err := parseOptions([]string{"--duplicates-config", "config.json"}, false); err == nil {
		t.Fatal("accepted duplication configuration outside audit")
	}
}

func TestQuietIsAnAuditFlag(t *testing.T) {
	for _, args := range [][]string{{"--quiet"}, {"--quiet=true"}} {
		_, _, err := parseOptions(args, false)
		if err == nil {
			t.Fatalf("compare accepted %v", args)
		}
	}

	if _, _, err := parseOptions([]string{"--quiet=true"}, true); err == nil {
		t.Fatal("quiet accepted a value")
	}
}
