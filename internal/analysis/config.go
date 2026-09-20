package analysis

import "github.com/kellen-miller/deburr/internal/report"

const (
	AnalyzerID      = "deburr/go"
	AnalyzerVersion = "0.2.0"

	highComplexityThreshold = 10
	duplicationTool         = "cpd"
	duplicationToolVersion  = "5.3.0"
	duplicationMinTokens    = 80
	duplicationMinLines     = 8
	duplicationThreshold    = "100"
)

// Config controls native Go discovery and metrics. The zero value uses the
// safe defaults: tests are included for separate accounting, while testdata,
// vendor, and generated code are covered but excluded from measurements.
type Config struct {
	Duplication      DuplicationConfig
	MaxFileBytes     int64
	ExcludeTests     bool
	IncludeTestdata  bool
	IncludeVendor    bool
	IncludeGenerated bool
}

// DuplicationConfig is intentionally opt-in. Native Go metrics do not depend
// on an external detector.
type DuplicationConfig struct {
	Tool      string
	Version   string
	Threshold string
	MinTokens int
	MinLines  int
	Requested bool
}

func DefaultConfig() Config {
	return Config{}
}

func (c Config) normalized() Config {
	if !c.Duplication.Requested {
		return c
	}

	if c.Duplication.Tool == "" {
		c.Duplication.Tool = duplicationTool
	}

	if c.Duplication.Version == "" {
		c.Duplication.Version = duplicationToolVersion
	}

	if c.Duplication.MinTokens <= 0 {
		c.Duplication.MinTokens = duplicationMinTokens
	}

	if c.Duplication.MinLines <= 0 {
		c.Duplication.MinLines = duplicationMinLines
	}

	if c.Duplication.Threshold == "" {
		c.Duplication.Threshold = duplicationThreshold
	}

	return c
}

func (c Config) reportSummary() report.ConfigSummary {
	tool := ""
	if c.Duplication.Requested {
		tool = toolLabel(c.Duplication.Tool)
	}

	return report.ConfigSummary{
		Language:                "go",
		IncludeTests:            !c.ExcludeTests,
		IncludeTestdata:         c.IncludeTestdata,
		IncludeVendor:           c.IncludeVendor,
		IncludeGenerated:        c.IncludeGenerated,
		MaxFileBytes:            c.MaxFileBytes,
		HighComplexityThreshold: highComplexityThreshold,
		Rules: []report.RuleConfig{
			{ID: "redundant-boolean-return", Enabled: true},
			{ID: "duplicate-branches", Enabled: true},
		},
		Duplication: report.DuplicationConfig{
			Requested: c.Duplication.Requested,
			Tool:      tool,
			Version:   c.Duplication.Version,
			Scope:     "production,test",
			MinTokens: c.Duplication.MinTokens,
			MinLines:  c.Duplication.MinLines,
			Threshold: c.Duplication.Threshold,
		},
	}
}

func (c Config) reportScope() report.Scope {
	return report.Scope{
		Languages:        []string{"go"},
		PathMode:         "source-relative",
		IncludeTests:     !c.ExcludeTests,
		IncludeTestdata:  c.IncludeTestdata,
		IncludeVendor:    c.IncludeVendor,
		IncludeGenerated: c.IncludeGenerated,
	}
}
