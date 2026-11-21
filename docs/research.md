# Research notes

These notes record external references and design decisions behind Deburr's
deterministic Go and TypeScript analyzer. The CLI owns analysis and report
generation, and the cleanup guide consumes its evidence without invoking a
model or applying automatic refactors.

## Current boundary

Deburr is a single Rust binary that analyzes Go and TypeScript. Its reports
preserve raw measurements, source locations, coverage, and compatibility
identities so a later comparison can distinguish a code change from a
measurement change. Findings, complexity, signals, and clones share function
identities, so they can be reviewed together.

## Why Rust, and why clones are built in

The Go version shelled out to a caller-installed jscpd and carried about 2,200
lines that existed only to drive it: snapshot directories, a reimplementation
of its config and `.gitignore` handling, version checks, and path mapping.
jscpd 5 is written in Rust and publishes MIT-licensed library crates, so a Rust
Deburr can find clones in-process, on the files it already parsed.

The split of ownership is deliberate. Deburr chooses files, parses them, and
reports. The `cpd-core` library does one thing: find repeated runs in token
streams that Deburr hands it. Deburr therefore does not read `.jscpd.json` and
scans nothing outside its analyzed files.

Findings from the crate investigation that shaped the design:

- The tokenizer crate does not use tree-sitter. Its Go and Rust support is a
  hand-written line scanner (keywords become identifiers, Rust lifetimes are
  mis-lexed, comment text becomes tokens). Deburr feeds `cpd-core` tokens from
  its own tree-sitter parse and does not depend on the tokenizer crate.
- The crates change public struct fields in almost every 0.1.x patch release.
  Deburr pins `cpd-core` exactly, calls it from one module, and pins results
  with a snapshot test.
- `cpd-core`'s line minimum is exclusive and depends on file order, and jscpd's
  percentage can exceed 100 and moves with whitespace. Deburr applies its own
  line minimum in code lines and reports the share of distinct code lines
  covered, so percentages are bounded and stable. They are not expected to
  match a standalone jscpd run.
- Reports from the Go and Rust versions are never compared: `compare` already
  refused different analyzer identities, so the parity bar is a semantic diff
  of files, functions, metrics, and findings rather than identical bytes.
  Byte identity would have meant reimplementing `go/printer`, `go/parser`
  messages, Go's JSON encoding, and `html/template` for no user benefit.

Two tree-sitter grammars are vendored and patched where upstream releases lag
the languages (TypeScript import types and Go generic methods). Both are
carried until upstream releases a fix.

## Sources

- [Measuring the sloppiness of code](https://earendil.com/posts/measuring-code-sloppiness/)
  motivates tracking degradation during iterative agent development.
- [SlopCodeBench](https://arxiv.org/html/2603.24755v1) describes iterative tasks
  and two structural signals: verbosity and erosion. Its reported experiments
  evaluate Python; Go rules require independent validation.
- [Bias in the Loop](https://arxiv.org/pdf/2604.16790) studies the sensitivity of
  code judgments to prompt presentation. Deburr's CLI uses deterministic
  analysis rather than model judgments.
- [ast-grep](https://ast-grep.github.io/) is the structural pattern tool used by
  SlopCodeBench's verbosity rules. Its pattern approach informed the research;
  Deburr does not depend on ast-grep.
- [Trellis](https://github.com/jayminwest/trellis) is a deterministic
  TypeScript/TSX analyzer with function-level erosion, duplication, import
  cycles, report comparisons, and a portable cleanup guide. Its metric and
  workflow design informed Deburr's design; it is not a runtime dependency.
- [jscpd](https://github.com/kucherenko/jscpd) 5 publishes the `cpd-core`
  library crate that Deburr links to find clones. It runs in-process on
  Deburr's own tokens, is pinned to an exact version, and its detection
  settings (minimum tokens and lines) are recorded in the report.
- [tree-sitter](https://tree-sitter.github.io/) is the parser for Go and
  TypeScript, so each file is parsed once for metrics, findings, and clones.

## Lessons from Trellis

- Preserve the measurements and source locations behind ranked findings.
- Report incomplete analysis and excluded source categories explicitly.
- Record analyzer and configuration identities so changing measurement
  semantics cannot silently appear as a code improvement.
- Keep the cleanup guide canonical and harness-neutral. Require justified
  changes and behavior verification; a lower metric alone is insufficient.
- Keep measurements separate from optional CI acceptance policy.

References inspected: [erosion implementation](https://github.com/jayminwest/trellis/blob/main/src/metrics/erosion.ts),
[complexity implementation](https://github.com/jayminwest/trellis/blob/main/src/metrics/complexity.ts),
[comparison compatibility](https://github.com/jayminwest/trellis/blob/main/src/compare/compatibility.ts),
[cleanup guide](https://github.com/jayminwest/trellis/blob/main/src/guides/cleanup.ts),
and [optional analyzer integration](https://github.com/jayminwest/trellis/blob/main/docs/quality-evidence.md).

Trellis labels its composite scoring formula provisional. Deburr leads with
findings, raw measurements, and changes over time rather than adopting those
weights. Its [calibration notes](https://github.com/jayminwest/trellis/blob/main/docs/count-calibration.md)
also demonstrate why detector settings must accompany comparisons.

## Measurement context

SlopCodeBench defines verbosity as the fraction of lines covered by structural
pattern findings or code clones, counting overlapping lines once. Erosion is
the share of complexity mass in functions with cyclomatic complexity above 10;
function mass is cyclomatic complexity multiplied by the square root of source
lines of code.

These signals inform the report design; they are not universal quality
thresholds. Deburr preserves local evidence and raw counts alongside aggregate
values. Use repository checks to verify behavior after cleanup; lower
measurements alone do not establish that a refactor is correct or easier to
maintain.
