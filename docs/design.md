# Go analyzer and report contract

This document defines the first native analyzer contract. The Go analyzer is
deterministic and reads source files without executing target code. Renderers
and the CLI consume `internal/report` values; they do not need to know how Go
syntax is traversed.

## API ownership

`internal/analysis` owns filesystem discovery, Go parsing, source metrics, and
verbosity findings. Its public entry point is:

```go
func Analyze(ctx context.Context, root string, cfg Config) (report.Report, error)
```

`root` is a file or directory. Relative paths in the report are relative to
the cleaned root directory (or to the parent directory when root is a file).
The analyzer never follows symlinks. A symlink encountered during traversal is
recorded as excluded coverage. Input paths are cleaned for traversal but are
never emitted as absolute paths.

The fixed traversal exclusions `.git`, `.hg`, `.svn`, `node_modules`, `.venv`,
`venv`, `.agent`, and `.worktrees` are skipped as dependency, metadata, or
agent-state directories. Their count is recorded in `Coverage` so changing
the analyzer version is required before changing this source-selection rule.

`Config` contains only analyzer behavior that can affect measurements or
coverage. Defaults are applied before analysis and the complete canonical
configuration is copied into `report.ConfigSummary`, so reports can be
compared safely. Version and rule changes change `Analyzer.Version` or the
configuration identity; a comparison must reject incompatible reports.

The initial native engine has no third-party dependency and does not invoke a
model, `go list`, `go test`, or target binaries. Duplication is an explicit,
separate request. The supported adapter is pinned to CPD 5.3.0 with an
80-token minimum, an 8-line minimum, and a fixed advisory 100% threshold. If
requested, the report records the selected executable, version, arguments,
source scope (`production,test`), and result status. The threshold does not
turn measurement into a quality gate; a nonzero tool exit still makes the run
incomplete. An unrequested detector is represented as `not_requested`, never
as a zero measurement.

## Report values

`internal/report.Report` is the renderer-neutral schema:

```go
type Report struct {
    SchemaVersion string
    Analyzer      AnalyzerIdentity
    Config        ConfigSummary
    Scope         Scope
    Coverage      Coverage
    Files         []File
    Functions     []Function
    Findings      []Finding
    Metrics       Metrics
    Duplication   *Duplication
    Review        *ReviewLedger
    Provenance    Provenance
}
```

All slices are sorted before returning: files by source-relative path,
functions by path/start/name/ID, and findings by path/start/rule/ID. Maps are
not part of the stable JSON surface. No timestamp, host path, or process value
is emitted. JSON renderers must use the schema's field names and preserve zero
values needed to explain empty or incomplete scans.

Schema 2 adds an optional review ledger. `review init` creates a complete,
unreviewed inventory of findings, high-complexity functions, and measured
clones. `review apply` validates an external ledger against the report identity
and item fingerprints, keeps missing items unreviewed, and exposes a changed
item's prior decision as stale. A decided item carries a reason category,
evidence, and an upstream owner when its category is `upstream`. The ledger is
review accounting; it does not suppress findings or decide whether a change is
correct.

An item marked `identity_ambiguous` may carry a decided review only when the
ledger's `source_manifest_digest` is known and matches the current report's
source manifest. A changed or unknown manifest leaves that item unreviewed and
preserves the prior decision as stale. Named, unambiguous items continue to
match by fingerprint when their source manifest changes. Structural signals are
syntactic counters: flat guards inspect direct function-body guards, field
mappings count field-copy-shaped keyed literals, nested branches count nested
branch syntax, and assertion-like calls use recognized test/assertion call
forms. They do not infer semantics. Clone `FamilyID` values identify connected
components joined by exact shared normalized source regions; each measured
pair remains an individual review item and the raw pair count is preserved.

`Provenance` keeps the source content manifest separate from `SourceIdentity`
Git commit/dirty state and `BuildIdentity` revision/modified state. `Known` and
`GitKnown` distinguish unavailable metadata from a known clean or modified
state; provenance is evidence and does not turn a dirty tree into a scan
failure.

`File` records every discovered source candidate, including exclusions and
failures:

```go
type File struct {
    Path     string
    Category Category // production, test, testdata, generated, vendor, unsupported
    Status   FileStatus // analyzed, excluded, unsupported, read_error, parse_error
    Bytes    int64
    Lines    int
    CodeLines int
    Error    string
}
```

`_test.go` files are `test`. A file under a `testdata` path component is
`testdata`; it is included in coverage and is excluded from production/test
metrics by default. A path component named `vendor` is `vendor` and excluded.
Generated code is excluded only when the source begins with the Go convention
`^// Code generated .* DO NOT EDIT\.$` before its first non-comment,
non-blank line. A non-Go regular file is `unsupported`. Symlinks and excluded
directories are counted in coverage but are never traversed or read.

`Function` preserves raw measurements and source evidence:

```go
type Function struct {
    ID               string
    Path             string
    Category         Category
    Name             string
    Start, End       Position
    SLOC             int
    Cyclomatic       int
    MaxNesting       int
    Mass             float64 // Cyclomatic * sqrt(SLOC)
    HighComplexity   bool    // Cyclomatic > 10
}
```

Top-level functions and methods use their declared names. Function literals
use stable ordinal names such as `Handle.func1` and `Handle.func1.func1` in
source order. Nested literals are separate functions. Their source lines are
owned by the innermost function for SLOC aggregation, so a physical line is
never added twice to a file or category total. A function's SLOC is the count
of non-comment, non-blank physical lines assigned to it. Function and
duplication positions are one-based line/column pairs; end positions are
inclusive.

Cyclomatic complexity starts at 1 and adds one for `if`, `for`, `range`,
`case` (excluding `default`), `&&`, `||`, and each non-default `select` case.
The `else` branch does not add a second decision. `switch` and type-switch
cases are counted as branches. This is a deliberately small, documented Go
rule set whose raw values remain available for future calibration.

`MaxNesting` is the maximum active nesting depth of control-flow constructs
(`if`, `for`, `range`, `switch`, type-switch, and `select`) in the function;
function literals are separate roots and do not increase their enclosing
function's depth. Aggregate buckets retain the maximum value observed.

`Metrics` contains aggregate raw values for the whole scan and separate
production/test buckets. Mass is the sum of function mass. High-complexity
mass is the sum for functions with complexity above 10, and its share is
`HighComplexityMass / Mass`; the share is null when mass is unavailable or
zero. File code lines are
physical non-comment, non-blank lines counted once. No composite score is
computed.

`Finding` is an evidence-backed review candidate. The initial rules are
conservative and structural: redundant boolean returns require a direct
comparison condition, literal boolean returns, and an explicitly declared
`bool` result; duplicated branch bodies require identical AST shape. These
checks identify review candidates and preserve the condition expression in
the suggested change. They do not prove a rewrite semantics-preserving: Go
types, effects, panics, and repository conventions still require review. A
finding contains a stable ID, location, rule, severity, message, and
machine-readable details. Rules may be expanded only with representative
behavioral tests and calibration evidence.

`Coverage` includes discovered, analyzed, excluded, unsupported, read-error,
and parse-error counts, plus per-category counts. A scan with no analyzable Go
files is successful only as a report-producing operation: its coverage states
that zero files were analyzed. Any read or parse failure returns a non-nil
error alongside the report and leaves a corresponding `File` entry. Thus a
partial scan cannot appear clean by omission.

## Comparison identity

Consumers compare `SchemaVersion`, analyzer ID/version, canonical config,
language, and scope before comparing findings or measurements. A changed rule,
threshold, exclusion, generated-file policy, or source-selection semantics is
incompatible. Added and deleted source files are normal comparison inputs and
do not make otherwise compatible reports invalid. Missing files and
parse/read failures are reported as coverage changes; they must not be
interpreted as resolved findings. Finding IDs use the source-relative path,
owning function identity, rule, normalized finding content, and occurrence.
Line moves can therefore change IDs when the owning function or occurrence
changes; compare consumers should show that as a new/resolved candidate with
the source evidence attached.
