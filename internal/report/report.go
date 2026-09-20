// Package report defines the stable, renderer-neutral deburr report schema.
package report

const SchemaVersion = "2"

type Report struct {
	Duplication   *Duplication     `json:"duplication,omitempty"`
	Review        *ReviewLedger    `json:"review,omitempty"`
	Provenance    Provenance       `json:"provenance"`
	Analyzer      AnalyzerIdentity `json:"analyzer"`
	SchemaVersion string           `json:"schema_version"`
	Files         []File           `json:"files"`
	Functions     []Function       `json:"functions"`
	Findings      []Finding        `json:"findings"`
	Scope         Scope            `json:"scope"`
	Metrics       Metrics          `json:"metrics"`
	Config        ConfigSummary    `json:"config"`
	Coverage      Coverage         `json:"coverage"`
}

type Provenance struct {
	Source SourceIdentity `json:"source"`
	Build  BuildIdentity  `json:"build"`
}

type BuildIdentity struct {
	Revision string `json:"revision"`
	Modified bool   `json:"modified"`
	Known    bool   `json:"known"`
}

type SourceIdentity struct {
	Commit         string `json:"commit"`
	ManifestDigest string `json:"manifest_digest"`
	Dirty          bool   `json:"dirty"`
	GitKnown       bool   `json:"git_known"`
}

type AnalyzerIdentity struct {
	ID      string `json:"id"`
	Version string `json:"version"`
}

// ConfigSummary contains every setting that can alter native measurements or
// coverage. Slices are kept ordered by the producer.
type ConfigSummary struct {
	Rules                   []RuleConfig      `json:"rules"`
	Language                string            `json:"language"`
	Duplication             DuplicationConfig `json:"duplication"`
	MaxFileBytes            int64             `json:"max_file_bytes"`
	HighComplexityThreshold int               `json:"high_complexity_threshold"`
	IncludeTests            bool              `json:"include_tests"`
	IncludeTestdata         bool              `json:"include_testdata"`
	IncludeVendor           bool              `json:"include_vendor"`
	IncludeGenerated        bool              `json:"include_generated"`
}

type RuleConfig struct {
	ID      string `json:"id"`
	Enabled bool   `json:"enabled"`
}

type Scope struct {
	PathMode         string   `json:"path_mode"`
	Languages        []string `json:"languages"`
	IncludeTests     bool     `json:"include_tests"`
	IncludeTestdata  bool     `json:"include_testdata"`
	IncludeVendor    bool     `json:"include_vendor"`
	IncludeGenerated bool     `json:"include_generated"`
}

type Coverage struct {
	ByCategory          []CategoryCoverage `json:"by_category"`
	Discovered          int                `json:"discovered"`
	Analyzed            int                `json:"analyzed"`
	Excluded            int                `json:"excluded"`
	Unsupported         int                `json:"unsupported"`
	ReadErrors          int                `json:"read_errors"`
	ParseErrors         int                `json:"parse_errors"`
	Symlinks            int                `json:"symlinks"`
	ExcludedDirectories int                `json:"excluded_directories"`
}

type CategoryCoverage struct {
	Category    Category `json:"category"`
	Discovered  int      `json:"discovered"`
	Analyzed    int      `json:"analyzed"`
	Excluded    int      `json:"excluded"`
	Unsupported int      `json:"unsupported"`
	ReadErrors  int      `json:"read_errors"`
	ParseErrors int      `json:"parse_errors"`
}

type Category string

const (
	CategoryProduction  Category = "production"
	CategoryTest        Category = "test"
	CategoryTestdata    Category = "testdata"
	CategoryGenerated   Category = "generated"
	CategoryVendor      Category = "vendor"
	CategoryUnsupported Category = "unsupported"
)

type FileStatus string

const (
	FileAnalyzed    FileStatus = "analyzed"
	FileExcluded    FileStatus = "excluded"
	FileUnsupported FileStatus = "unsupported"
	FileReadError   FileStatus = "read_error"
	FileParseError  FileStatus = "parse_error"
)

type File struct {
	Path      string     `json:"path"`
	Category  Category   `json:"category"`
	Status    FileStatus `json:"status"`
	Reason    string     `json:"reason,omitempty"`
	Error     string     `json:"error,omitempty"`
	Bytes     int64      `json:"bytes"`
	Lines     int        `json:"lines"`
	CodeLines int        `json:"code_lines"`
}

type Position struct {
	Line   int `json:"line"`
	Column int `json:"column"`
}

type Function struct {
	ID                string            `json:"id"`
	Fingerprint       string            `json:"fingerprint,omitempty"`
	Path              string            `json:"path"`
	Category          Category          `json:"category"`
	Name              string            `json:"name"`
	StructuralSignals StructuralSignals `json:"structural_signals"`
	End               Position          `json:"end"`
	Start             Position          `json:"start"`
	SLOC              int               `json:"sloc"`
	Cyclomatic        int               `json:"cyclomatic"`
	MaxNesting        int               `json:"max_nesting"`
	Mass              float64           `json:"mass"`
	HighComplexity    bool              `json:"high_complexity"`
	IdentityAmbiguous bool              `json:"identity_ambiguous"`
}

type StructuralSignals struct {
	FlatGuards         int `json:"flat_guards"`
	FieldMappings      int `json:"field_mappings"`
	NestedBranches     int `json:"nested_branches"`
	AssertionLikeCalls int `json:"assertion_like_calls"`
}

type Metrics struct {
	All        MetricBucket `json:"all"`
	Production MetricBucket `json:"production"`
	Test       MetricBucket `json:"test"`
}

type MetricBucket struct {
	HighComplexityMassShare *float64 `json:"high_complexity_mass_share"`
	CodeLines               int      `json:"code_lines"`
	Functions               int      `json:"functions"`
	Cyclomatic              int      `json:"cyclomatic"`
	MaxNesting              int      `json:"max_nesting"`
	Mass                    float64  `json:"mass"`
	HighComplexityFunctions int      `json:"high_complexity_functions"`
	HighComplexityMass      float64  `json:"high_complexity_mass"`
}

type Finding struct {
	ID                string   `json:"id"`
	Fingerprint       string   `json:"fingerprint,omitempty"`
	Rule              string   `json:"rule"`
	Path              string   `json:"path"`
	Severity          string   `json:"severity"`
	Message           string   `json:"message"`
	Details           []Detail `json:"details,omitempty"`
	Start             Position `json:"start"`
	End               Position `json:"end"`
	IdentityAmbiguous bool     `json:"identity_ambiguous"`
}

type Detail struct {
	Key   string `json:"key"`
	Value string `json:"value"`
}

type DuplicationConfig struct {
	Tool      string `json:"tool,omitempty"`
	Version   string `json:"version,omitempty"`
	Scope     string `json:"scope,omitempty"`
	Threshold string `json:"threshold,omitempty"`
	MinTokens int    `json:"min_tokens,omitempty"`
	MinLines  int    `json:"min_lines,omitempty"`
	Requested bool   `json:"requested"`
}

type DuplicationStatus string

const (
	DuplicationNotRequested DuplicationStatus = "not_requested"
	DuplicationMeasured     DuplicationStatus = "measured"
	DuplicationError        DuplicationStatus = "error"
)

type Duplication struct {
	Status DuplicationStatus `json:"status"`
	Error  string            `json:"error,omitempty"`
	Clones []Clone           `json:"clones,omitempty"`
	Config DuplicationConfig `json:"config"`
}

type Clone struct {
	ID                string     `json:"id"`
	FamilyID          string     `json:"family_id"`
	Fingerprint       string     `json:"fingerprint,omitempty"`
	Category          Category   `json:"category"`
	Locations         []Location `json:"locations"`
	Tokens            int        `json:"tokens"`
	Lines             int        `json:"lines"`
	IdentityAmbiguous bool       `json:"identity_ambiguous"`
}

type Location struct {
	Path  string   `json:"path"`
	Start Position `json:"start"`
	End   Position `json:"end"`
}
