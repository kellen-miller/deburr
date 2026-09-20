# Deburr cleanup guide

Use this guide with any harness that can run the `deburr` binary. Deburr is a
deterministic Go analyzer. It reports evidence for review; it does not use a
model, rewrite files, or decide that a repository is good or bad.

## Before changing code

Build or obtain the exact CLI revision you will use. Do not claim that an
audit ran until the binary is available and the command exits with a report.

Read the target repository's instructions before editing. Inspect its current
status and diff, and preserve existing user changes. The guide does not grant
permission to overwrite unrelated work.

Create a harness-owned report directory outside the scanned path. Keep the
original baseline for the whole cleanup session and use a new filename for
each iteration:

```text
REPORT_DIR=<temporary-directory-outside-PATH>
deburr audit PATH --format json --output "$REPORT_DIR/before.json"
```

Read the report's analyzer identity, configuration identity, scope, coverage,
unsupported files, parse failures, source categories, findings, and raw
measurements. An incomplete scan is evidence of incomplete coverage, not a
clean result. Keep generated, vendored, fixture, test, and production code
distinctions visible when deciding what to inspect.

Create the external review ledger from that immutable baseline:

```text
deburr review init "$REPORT_DIR/before.json" --output "$REPORT_DIR/review.json"
```

The ledger accounts for every finding, high-complexity function, and measured
clone. Review each item with one of `unreviewed`, `refactored`, `retained`, or
`deferred`. A decided item needs a concrete reason category, source or test
evidence, and an upstream owner when the category is `upstream`. The ledger is
accounting evidence, not an automatic judgment or a suppression list; leave
uncertain items `unreviewed`.

Scope accounting follows the requested review boundary. If the task names a
bounded directory, package, production/test category, or report subset, record
that scope and do not silently expand it to the whole repository. Useful
reason categories include `cohesive-validation`, `lifecycle-ordering`,
`distinct-semantics`, `generated-contract`, `scenario-clarity`, and
`low-benefit`, plus `upstream`. A category is only a label: the decision still
needs item-specific reasoning and evidence. A schema or generated API issue
belongs in `deferred` with `reason_category: upstream`, the responsible owner,
and concrete evidence while it is unresolved.

Apply decisions to the same baseline when producing a human-readable review:

```text
deburr review apply "$REPORT_DIR/before.json" \
  --ledger "$REPORT_DIR/review.json" \
  --format html --output "$REPORT_DIR/before-reviewed.html"
```

Keep the baseline report, ledger, exact analyzer/configuration identity, source
scope, and optional duplication tool settings together. A later audit must use
matching settings before it can support a comparison. Changed source makes a
prior item stale through its fingerprint; removed or newly discovered IDs need
deliberate reconciliation, and a missing item never proves that work is done.
Preserve the original ledger and record fresh decisions for changed items.

Also inspect source and build provenance. The source record keeps the analyzed
content manifest separate from the Git commit and dirty-tree state; the build
record identifies the exact Deburr revision and toolchain used to produce the
report. A dirty tree is evidence to retain with the report, not permission to
substitute a different source or build.

## Cleanup loop

1. Choose one bounded native finding, complexity hotspot, or duplication clone
   pair with source locations and enough context to understand its behavior.
2. Decide whether the finding identifies a real maintainability improvement.
   Preserve the existing behavior and local ownership boundaries. Do not add a
   helper, wrapper, or abstraction only to improve a measurement.
3. Make the smallest readable change that has a reason beyond a lower number.
   Deburr does not apply automatic refactors.
4. Run the repository's relevant formatter, linter, type checker, build, and
   behavioral tests. Use the repository's normal checks as the behavior gate.
5. Audit the same scope and settings again:

   ```text
   deburr audit PATH --format json --output "$REPORT_DIR/after-001.json"
   deburr compare "$REPORT_DIR/before.json" "$REPORT_DIR/after-001.json" --format text
   ```

6. Review the changed findings, raw measurements, coverage, review counts, and
   compatibility metadata together with the test evidence. A lower measurement
   alone does not establish an improvement. Treat an incompatible comparison as
   a reason to rerun with matching analyzer, configuration, source-scope, and
   duplication identities. Keep the original baseline and use `after-002.json`,
   `after-003.json`, and so on for later iterations.

When the baseline has decisions, render its reviewed JSON before comparing:

```text
deburr review apply "$REPORT_DIR/before.json" \
  --ledger "$REPORT_DIR/review.json" \
  --format json --output "$REPORT_DIR/before-reviewed.json"
deburr compare "$REPORT_DIR/before-reviewed.json" "$REPORT_DIR/after-001.json" \
  --format text
```

If changed source makes an item stale, inspect the preserved prior decision,
review the current source again, and write a fresh decision with the current
fingerprint and `stale: false` (or omit the field). Applying that fresh decision
clears `stale`; alternatively, `review init` on the current report starts a new
ledger and intentionally discards its prior decisions. Retain the old baseline
ledger or annotated report as history rather than overwriting it.

Repeat for the next bounded finding. Stop when the remaining findings have no
justified improvement, when the scan is incomplete, or when behavior evidence
does not support the proposed change. Do not chase zero findings.

## Report and CI boundaries

The pre-1.0 CLI supports Go analysis only. A report must make unsupported or
unparsed input visible. Report formats are `text`, `json`,
`html`, and `github`:

```text
deburr audit PATH --format text --output report.txt
deburr audit PATH --format html --output report.html
```

Use `github` when a caller wants GitHub Actions annotations or a file suitable
for an artifact. The GitHub Action invokes this same CLI and exposes its report
path. It does not add an independent analyzer or mandatory quality threshold.

Duplication analysis is opt-in. The supported adapter uses pinned CPD 5.3.0,
an 80-token and 8-line minimum, and an advisory 100% threshold. When it is
enabled, keep its status, engine, version, and settings in the report so a
later comparison cannot confuse a detector change with a code change. A
nonzero tool exit makes the run incomplete; the threshold does not act as a
quality gate. The default status is not requested.

The GitHub Action keeps the same opt-in boundary. Set `duplicates: true` and,
when CPD is not on `PATH`, pass `cpd` with the path to a caller-provisioned
CPD 5.3.0 executable. The action does not install a second detector or infer
that duplication should run from the presence of a path.

Report positions, including duplication locations, use one-based lines and
columns with inclusive end positions.

The CLI has no composite score. Read the findings, measurements, and coverage
that the report defines, then use engineering judgment and repository checks
to decide what to keep.
