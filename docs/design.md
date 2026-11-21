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
| `syntax` | tree-sitter parsing, code tokens, line maps, and the per-language node-kind tables (`go.rs`, `typescript.rs`, `rust.rs`) |
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
agent-state directories. A directory named `target` is skipped the same way
when it holds a regular file named `CACHEDIR.TAG`, which Cargo writes into its
output directory; a `target` directory without the tag is ordinary source and
is walked. Skipped directories are counted in `Coverage`, so changing the
analyzer version is required before changing this source-selection rule.

## Parsing

Every file is parsed once with tree-sitter (`tree-sitter-go`,
`tree-sitter-typescript` for `.ts`, `.mts`, `.cts`, and its `tsx` grammar for
`.tsx`, `tree-sitter-rust` for `.rs`). The parse result feeds line counting, function discovery, findings,
signals, clone identity, and duplication tokens. No language runtime, compiler,
or model is invoked, and nothing runs `go list`, `go test`, `tsc`, `cargo`,
`rustc`, or target binaries.

A file is a parse error if its tree contains an `ERROR` or `MISSING` node. For
Go, the structural check also applies: the first non-comment top-level child
must be a `package_clause`, and every top-level child must be a package clause,
import, function, method, type, var, or const declaration, or a comment. The
message is `syntax error at LINE:COLUMN`. Go string and rune literals are
also checked against the Go spec (valid escapes, octal values up to 255, no
surrogate or out-of-range `\u`/`\U` code points, one character per rune
literal), because tree-sitter accepts some literals `go/parser` rejects.

The Go and TypeScript grammars come from forks with small patches that
upstream releases lack: import types and three smaller gaps in TypeScript, and
generic methods and `new(expr)` in Go. `Cargo.toml` pins each fork's
`deburr-patches` branch by commit
([tree-sitter-typescript](https://github.com/kellen-miller/tree-sitter-typescript/blob/deburr-patches/PATCHES.md), [tree-sitter-go](https://github.com/kellen-miller/tree-sitter-go/blob/deburr-patches/PATCHES.md)). Each
`PATCHES.md` records the upstream version, the change, and how to regenerate
the parser. To update a grammar, push a new commit to the fork branch, change
the `rev`, and rerun the grammar tests and the VS Code corpus check. The Rust
grammar is the unpatched crates.io `tree-sitter-rust` 0.24 release: it parsed
every `.rs` file in ten local Rust repositories without an `ERROR` or
`MISSING` node, so no fork is needed.

### Code tokens

A code token is a tree-sitter leaf that is not a comment. String-like nodes
(Go interpreted and raw strings and runes; TypeScript strings, templates, and
regular expressions; Rust string, raw string, and character literals) are
single tokens. After each statement of a statement container, the token list
gets a synthetic `;` terminator at the statement's end (Rust's containers are
the file, `block`, and `declaration_list`). Go and TypeScript insert implicit
semicolons at line ends, so without terminators `return` followed by a newline
and `g()` would look identical to `return g()`. Rust also gets
terminators, so statement boundaries are uniform across languages. Terminators take part in fingerprints, branch equality, clone
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
    pub schema_version: String,       // "4"
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
`"4"`, a non-empty analyzer, language, and scope.

Schema 3 replaced schema 2 when the rewrite dropped the external clone
detector, so the duplication section changed shape. Schema 4 adds Rust: each
file's `inline_test_code_lines` and the four Rust counters in
`structural_signals` (`clone_like_calls`, `unwrap_like_calls`, `panic_macros`,
`unsafe_blocks`, always zero for Go and TypeScript), and `.rs` files stop being
`unsupported`. Decoding rejects schema 3, and `compare` refuses reports from
different schemas or analyzer versions. The analyzer version is the release
version (`DEBURR_VERSION` when the binary was built for a release, else the
Cargo package version), so no source edit bumps it.

`File` records every discovered source candidate, including exclusions and
failures: path, category (`production`, `test`, `testdata`, `generated`,
`vendor`, `unsupported`), status (`analyzed`, `excluded`, `unsupported`,
`read_error`, `parse_error`), reason, error, bytes, lines, code lines, and
inline test code lines (the code lines inside a Rust file's inline test
regions; zero for every other file).

`_test.go` files are `test`. A path component named `testdata` is `testdata`,
and a component named `vendor` is `vendor`. TypeScript `.test.*`, `.spec.*`,
and `__tests__` paths are `test`. A Rust `.rs` path with a `tests` or
`benches` directory component is `test`. Generated code is the Go convention
`// Code generated ... DO NOT EDIT.` before the first non-comment line (the
same line among a TypeScript or Rust file's leading `//` comments; a Rust
file's leading `// @generated` comment also counts). Other regular files
outside `.go`, `.ts`, `.tsx`, `.mts`, `.cts`, and `.rs` are `unsupported`.
Build constraints and Cargo features are not evaluated.

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
nests. (Rust differs on both counts; see [Rust rules](#rust-rules).) Maximum
nesting is the deepest level of control-flow constructs. Nested function units
are separate roots. Mass is cyclomatic complexity times the
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
branches, and Rust records nested branches plus its four ownership and panic
counters. They do not infer semantics.

### Rust rules

`src/syntax/rust.rs` is the Rust table, written against tree-sitter-rust
0.24's node kinds; the engine is the same one Go and TypeScript use.

**Units and names.** A unit is a `function_item` with a body or any
`closure_expression`; a trait method signature without a body is not a unit.
IDs are `rust/<path>::<name>`. A name is the enclosing unit's name (if any),
then the `mod`, `impl`, and `trait` containers between it and the unit, then
its own name:

- `mod` and `trait` items contribute their names, joined with `::`;
- an inherent `impl` contributes its type (`Type::method`) and a trait `impl`
  contributes `<Type as Trait>` (`<Type as Trait>::method`), with generic
  arguments dropped from both;
- a closure is `{closure#N}` under its parent, N counting the closures that
  parent owns directly in source order from 1. A closure inside a `const` or
  `static` initializer is named under that item.

Repeated names get `#2`, `#3` and are identity-ambiguous, as in TypeScript.

**Complexity.** Cyclomatic complexity starts at 1 and adds one for each
`if_expression` (so `if let` and every `else if`), `while_expression` (so
`while let`), `for_expression`, `try_expression` (`?`), `match` arm unless its
whole pattern is `_`, match guard (so a guarded arm adds two, arm plus guard,
and `_ if guard` is a guarded arm, not the catch-all), `let ... else`, `&&` and `||`
(`binary_expression`), and each `&&` joining the conjuncts of an `if let`
chain. `loop` adds nesting only. Nesting constructs are `if`, `while`, `for`,
`loop`, and `match`. An `if` that is the body of an `else` clause stays at the
depth of the `if` that heads the chain, so `else if` chains do not deepen,
unlike TypeScript's.

**Signals.** `OwnershipRules` in the table drives four counters, taken inside
a function body without descending into nested function units. A
`call_expression` whose `function` is a `field_expression` is a method call:
`clone`, `to_owned`, and `cloned` with no arguments are clone-like; `unwrap`,
`unwrap_unchecked`, and `unwrap_err` with no arguments and `expect` and
`expect_err` with one argument are unwrap-like. A `macro_invocation` whose
path's last segment is `panic`, `todo`, `unimplemented`, or `unreachable`
counts as a panic macro, and each `unsafe_block` counts once. A bare function
call such as `clone(x)`, a method with other arguments, and a field named
`clone` are not counted. Nothing is typed or resolved.

**Findings.** Both rules use the conditional shape. `redundant-boolean-return`
needs an `if_expression` whose condition is a comparison `binary_expression`
(`==`, `!=`, `<`, `<=`, `>`, `>=`, one level of parentheses unwrapped) and
whose consequence and `else` block each hold a lone opposite
`boolean_literal`, as a tail expression or a `return`; the sibling form `if c
{ return true; }` followed by `false` also counts. A `boolean_literal`'s text
decides true or false. `duplicate-branches` needs an `else_clause` holding a
block whose code tokens equal the consequence's; an `else if` never counts.
The shared finding engine is `src/analysis/findings.rs`.

**Inline tests.** The table's `test_regions` hook returns the byte ranges of
test items in a non-test file. A run of attributes applies to the item after
it, and that item (attributes included) is a test region when an attribute is
`cfg(..)` naming `test` outside `not(..)`, or the item is a function and an
attribute's path ends in `test` (`#[test]`, `#[tokio::test]`). A region is
test code throughout; regions are not searched for nested ones. Every function
in a region has category `test`, and the file keeps category `production`.
`File.inline_test_code_lines` is the number of code lines inside regions.
`summarize_metrics` adds a file's `code_lines - inline_test_code_lines` to
production and `inline_test_code_lines` to test; `all` is unchanged. With
`--exclude-tests`, inline-test functions and findings are dropped, and inline
lines are counted in no bucket, `all` included.

**Macros.** tree-sitter-rust leaves the contents of `macro_rules!` bodies and
macro invocation arguments as token trees, not expressions. Code inside them
is therefore not measured (no complexity, signals, findings, or function
units); the invocation itself is still a node and is counted where it stands.
This is a known limitation, not something Deburr works around: it does not
expand macros.

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
A Rust production file with inline test regions contributes its tokens outside
the regions to the production group and its tokens inside them to the test
group, each with its own code lines and with real source positions, so
production and inline test code never pair, even within one file. With
`--exclude-tests` the inline tokens are not kept. Deburr feeds `cpd-core` tokens from its own tree-sitter parse rather than the
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
