# Analyzer and report contract

This document defines the contract of the Rust analyzer. The analyzer is
deterministic and reads source files without executing target code. Renderers,
`compare`, and `review` consume `report::Report` values; they do not need to
know how source syntax is traversed.

## Module ownership

Deburr is one Cargo package: a library and a thin binary (`src/main.rs` calls
`deburr::cli::run`). Each module hides one decision.

| Module | Owns |
| --- | --- |
| `cli` | Argument parsing, exit codes, progress lines, output-file safety |
| `audit` | One audit from start to finish: discovery, parallel analysis, duplication, summaries, provenance, review ledger, and every ordering rule |
| `discover` | Walking the target, categories, statuses, exclusion reasons, reading bytes |
| `syntax` | tree-sitter parsing, code tokens, line maps, and the per-language node-kind tables (`go.rs`, `typescript.rs`) |
| `analysis` | Function units, metrics, signals, and findings, written once against a language's rule table |
| `identity` | Every digest and ID: function, finding, clone, family, manifest, and report identity |
| `duplication` | The only module that imports `cpd-core` |
| `report` | The serialized schema and its validation |
| `review`, `compare` | The review ledger and report comparison, on `Report` values only |
| `render` | Text, JSON, HTML, and GitHub workflow-command output |
| `provenance` | The audited tree's Git state and the build's version |

Per-language knowledge lives in one table per language, so adding a language
means adding one rules file. Changing an ID scheme means editing
`identity.rs`. Upgrading `cpd-core` means editing `duplication.rs`.

`audit::audit(root, options, progress)` always returns a report plus a list of
errors. A non-empty error list means the CLI exits 1 after writing the report,
so a partial scan cannot appear clean by omission.

Relative paths in the report are relative to the cleaned root directory (or to
the parent directory when the root is a file). The analyzer never follows
symlinks; a symlink is recorded as excluded coverage. Input paths are never
emitted as absolute paths.

The fixed traversal exclusions `.git`, `.hg`, `.svn`, `node_modules`, `.venv`,
`venv`, `.agent`, and `.worktrees` are skipped as dependency, metadata, or
agent-state directories. Their count is recorded in `Coverage`, so changing the
analyzer version is required before changing this source-selection rule.

## Parsing

Every file is parsed once with tree-sitter (`tree-sitter-go`,
`tree-sitter-typescript` for `.ts`, `.mts`, `.cts`, and its `tsx` grammar for
`.tsx`). The parse result feeds line counting, function discovery, findings,
signals, clone identity, and duplication tokens. No language runtime, compiler,
or model is invoked, and nothing runs `go list`, `go test`, `tsc`, or target
binaries.

A file is a parse error if its tree contains an `ERROR` or `MISSING` node. For
Go, the structural check also applies: the first non-comment top-level child
must be a `package_clause`, and every top-level child must be a package clause,
import, function, method, type, var, or const declaration, or a comment. The
message is `syntax error at LINE:COLUMN`. Lexical validity of individual
literals is not re-checked.

The Go and TypeScript grammars are vendored under `third_party/` with small
patches (import types in TypeScript type positions, generic methods and
`new(expr)` in Go) that upstream releases lack. Each directory records the
upstream version and a `PATCHES.md`.

### Code tokens

A code token is a tree-sitter leaf that is not a comment. String-like nodes
(Go interpreted and raw strings and runes; TypeScript strings, templates, and
regular expressions) are single tokens. After each statement of a statement
container, the token list gets a synthetic `;` terminator at the statement's
end. Both languages insert implicit semicolons at line ends, so without
terminators `return` followed by a newline and `g()` would look identical to
`return g()`. Terminators take part in fingerprints, branch equality, clone
identity, and duplication input, but never in line counts.

`lines` counts newline-terminated lines (plus one when the last byte is not a
newline). A line is a code line when any code token touches it. Every file
that was read is tokenized for line counts, including excluded files and files
with syntax errors.

## Report values

`report::Report` is the renderer-neutral schema, serialized with `serde`. JSON
output is pretty-printed with a trailing newline.

```rust
pub struct Report {
    pub duplication: Option<Duplication>,
    pub review: Option<ReviewLedger>,
    pub provenance: Provenance,
    pub analyzer: AnalyzerIdentity,   // { id: "deburr", version }
    pub schema_version: String,       // "3"
    pub files: Vec<File>,
    pub functions: Vec<Function>,
    pub findings: Vec<Finding>,
    pub scope: Scope,
    pub metrics: Metrics,
    pub config: ConfigSummary,
    pub coverage: Coverage,
}
```

All lists are sorted before the report is returned: files by source-relative
path, functions by path/start/name/ID, findings by path/start/rule/ID, clones
by ID. Output never depends on thread timing. No timestamp, host path, or
process value is emitted. Decoding rejects unknown fields and requires schema
`"3"`, a non-empty analyzer, language, and scope.

Schema 3 replaces schema 2 and the analyzer version is `1.0.0`, so `compare`
refuses to mix reports from the Go versions. The duplication section changed
shape because the external detector is gone.

`File` records every discovered source candidate, including exclusions and
failures: path, category (`production`, `test`, `testdata`, `generated`,
`vendor`, `unsupported`), status (`analyzed`, `excluded`, `unsupported`,
`read_error`, `parse_error`), reason, error, bytes, lines, and code lines.

`_test.go` files are `test`. A path component named `testdata` is `testdata`,
and a component named `vendor` is `vendor`. TypeScript `.test.*`, `.spec.*`,
and `__tests__` paths are `test`. Generated code is the Go convention
`// Code generated ... DO NOT EDIT.` before the first non-comment line (the
same line among a TypeScript file's leading `//` comments). Other regular
files outside `.go`, `.ts`, `.tsx`, `.mts`, and `.cts` are `unsupported`.
Build constraints are not evaluated.

`Function` preserves raw measurements and source evidence: ID, fingerprint,
path, category, name, structural signals, start and end positions, SLOC,
cyclomatic complexity, maximum nesting, mass, a high-complexity flag, and an
identity-ambiguous flag. Positions are one-based line and byte-column pairs
with inclusive ends.

Top-level functions and methods use their declared names, with methods as
`Receiver.Name` (`(*T).Name` for pointer receivers). Function literals use
ordinal names such as `Handle.func1`. A repeated identity gets `#2`, `#3` in
source order and is marked identity-ambiguous. A function's SLOC is the count
of code lines assigned to it; a line belongs to the innermost function whose
range contains it, so nested closures never add a line twice.

Cyclomatic complexity starts at 1 and adds one for each `if`, loop, non-default
`case` and `select` case, `&&`, and `||`. TypeScript also counts `catch`,
ternaries, and `??`. An `else` does not add a second decision, but an `else if`
nests. Maximum nesting is the deepest level of control-flow constructs. Nested
function units are separate roots. Mass is cyclomatic complexity times the
square root of SLOC. A function is high complexity when its cyclomatic
complexity exceeds 10.

`Metrics` aggregates the whole scan and the production and test buckets. The
high-complexity mass share is null when mass is zero. No composite score is
computed.

`Finding` is an evidence-backed review candidate with a stable ID, location,
rule, severity, message, and machine-readable details. The rules are
conservative: `redundant-boolean-return` requires a comparison condition and
opposite boolean literal returns (Go also requires an explicit single `bool`
result), and `duplicate-branches` requires both branches to have identical code
tokens. Equality is by token text, so layout and comments never make two
branches differ. These findings do not prove a rewrite semantics-preserving.

Structural signals are syntactic counters: flat guards, field mappings, nested
branches, and assertion-like calls in Go tests. TypeScript records only nested
branches. They do not infer semantics.

`Coverage` counts discovered, analyzed, excluded, unsupported, read-error, and
parse-error files, overall and per category. A scan with no analyzable files is
a successful report whose coverage says zero files were analyzed. Any read or
parse failure is returned as an error alongside the report.

## Duplication

Duplication is on by default. `--no-duplicates` records status
`not_requested`; a skipped detector is never a zero measurement.

`src/duplication.rs` is the only module that imports `cpd-core` (pinned exactly
because its 0.1.x API changes between patch releases). Deburr owns file
selection, parsing, and reporting; `cpd-core` owns one job: finding repeated
runs in token streams that Deburr hands it. Production files and test files are
detected as two separate groups, so no clone pairs production with test code.
Deburr feeds `cpd-core` tokens from its own tree-sitter parse rather than the
tokenizer crate, because the tokenizer's Go handling is crude (keywords are
identifiers and comment text becomes tokens).

Settings are `--min-clone-tokens` (default 80) and `--min-clone-lines`
(default 8), recorded in the report and part of comparison identity.
`.jscpd.json` is not read. `cpd-core`'s own line minimum is bypassed because it
depends on file order and is off by one; Deburr keeps a clone only when both
endpoints each cover at least the minimum number of code lines.

The duplication section is `{ status, min_tokens, min_lines, categories,
clones }`, with status `measured`, `not_requested`, or `error`. Each category
carries its files, code lines, duplicated code lines, and percentage. Each
clone carries an ID, family ID, fingerprint, category, token count, an
identity-ambiguous flag, and two endpoints with a path, start, end, code-line
count, and `functions`: the IDs of the functions each endpoint overlaps (empty
when the region is file-level code). Every such ID is a function ID in the same
report. A category's percentage is the number of distinct code lines covered by
any clone endpoint divided by the category's code lines, times 100. It is
bounded by 100, ignores blank and comment edits, and deliberately differs from
jscpd's formula.

Clone fingerprints use the endpoints' token text and enclosing function
identity, so a clone keeps its ID when it moves within a file. Clone `family_id`
values identify connected components joined by shared endpoint identities; each
measured pair remains an individual review item.

## Review ledger

`review init` creates a complete, unreviewed inventory of findings,
high-complexity functions, and clones. `review apply` validates an external
ledger against the report identity and item fingerprints, keeps missing items
unreviewed, and exposes a changed item's prior decision as stale. A decided
item carries a reason category, evidence, and an upstream owner when its
category is `upstream`. The ledger is review accounting; it does not suppress
findings or decide whether a change is correct.

An item marked `identity_ambiguous` may carry a decided review only when the
ledger's `source_manifest_digest` is known and matches the current report's
manifest. Named, unambiguous items continue to match by fingerprint when the
manifest changes.

## Comparison identity

`compare` checks schema version, analyzer ID and version, canonical
configuration (including the duplication settings), language, and scope before
comparing findings, clones, or measurements. A changed rule, exclusion,
generated-file policy, source-selection semantics, or clone setting is
incompatible. Added and deleted files are normal inputs. Missing files and
parse or read failures are reported as coverage changes and are never read as
resolved findings. Finding, function, and clone matching runs by ID, then by
fingerprint, and ambiguous matches are reported as ambiguous.

`provenance` keeps the source content manifest separate from Git commit and
dirty state and from the build's version. Unavailable metadata is distinguished
from a known clean or modified state; provenance is evidence and never turns a
dirty tree into a scan failure.

## Release contract

The release workflow computes the next version without publishing, builds and
smoke-tests five targets with `DEBURR_VERSION` set to that version, writes one
`SHA256SUMS`, checks `scripts/install-deburr.sh` against the assembled
archives, and only then creates the release and attaches the assets. If the
upload fails, the release and tag are deleted so no tag exists without binaries.
The GitHub Action uses the same install script at release tags and builds from
source everywhere else.
