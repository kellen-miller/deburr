# Research notes

These notes record external references and design decisions behind Deburr's
current deterministic Go analyzer. The CLI owns analysis and report
generation, and the cleanup guide consumes its evidence without invoking a
model or applying automatic refactors.

## Current boundary

Deburr is experimental, pre-1.0, and Go-only. Its reports preserve raw
measurements, source locations, coverage, and compatibility identities so a
later comparison can distinguish a code change from a measurement change.

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
- [jscpd (CPD)](https://github.com/kucherenko/jscpd) powers Deburr's optional
  duplication analysis, pinned to CPD 5.3.0. It runs only when requested, uses
  caller-provided tooling, and records its identity and configuration in the
  report.

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
