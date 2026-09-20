# deburr

Find the rough edges that accumulate as code evolves.

Deburr is an experimental, pre-1.0 deterministic code-quality CLI written in
Go. It analyzes Go source and produces evidence for developers, coding agents,
and CI. It does not invoke models, rewrite files, or claim that code is good or
bad.

**Status:** `v0.1.0` is the initial experimental release. The analyzer
currently identifies itself as `deburr/go` version `0.2.0`, and reports use
schema `2`; those are analyzer and report compatibility versions, separate from
the CLI release version.

## Current scope

- Go-only analysis.
- Terminal text, JSON, HTML, and GitHub Actions report formats.
- A thin GitHub Action that builds and runs the same CLI.
- A harness-neutral cleanup skill with one canonical guide.
- Source locations, raw measurements, coverage, and comparable reports.
- Explicit production, test, generated, vendored, unsupported, and unparsed
  coverage where the analyzer can identify those categories.
- Optional duplication analysis, recorded with its status, engine, version,
  and settings when requested. It is not requested by default.

There is no model-based judgment, automatic refactoring, forced CI quality
gate, or composite sloppiness score. Findings are evidence for review,
not proof of AI authorship or poor design.

The native analyzer is parser-only: it discovers regular `.go` files under
the input, including files whose build constraints would exclude them from a
particular build. It does not type-check, invoke `go list`, execute target
code, or run a compiler. Configured category exclusions remain visible in
coverage.

## Install and run

Install the initial experimental release with:

```sh
go install github.com/kellen-miller/deburr/cmd/deburr@v0.1.0
```

To build the current source from a checkout, use Go from the version declared
by `go.mod`:

```sh
go install ./cmd/deburr
deburr audit ./path/to/repository --format text --output report.txt
deburr audit ./path/to/repository --format json --output report.json
deburr compare report-before.json report-after.json --format text
deburr review init report.json --output review.json
deburr review apply report.json --ledger review.json --format html --output reviewed.html
deburr guide cleanup
```

Duplication is an explicit CLI opt-in: `--duplicates` uses the pinned CPD
5.3.0 executable from `PATH`, while `--cpd PATH` selects an executable that
must report that same supported version. The adapter uses an 80-token,
8-line minimum and advisory threshold 100%. The action leaves duplication
off by default; reports record `not_requested` unless its `duplicates` input is
enabled.

`deburr guide cleanup` prints the canonical workflow from
[`internal/guide/cleanup.md`](internal/guide/cleanup.md). The CLI rejects audit
output inside a directory target (and rejects the target itself when auditing
a file). Keep reports in a harness-owned directory outside the scanned path,
retain one baseline, and use unique output files for later iterations.

The report records analyzer and configuration identity. Treat incompatible
comparisons as measurement changes that need a matching rerun. Read findings,
raw measurements, and coverage together with the repository's normal tests;
a lower measurement alone does not justify a refactor.

Reports also retain source and build provenance: the content manifest is
separate from Git commit and dirty-tree state, and the build record identifies
the exact Deburr revision and toolchain used for the scan.

Reports use schema 2 and can carry an external review ledger. `review init`
creates an unreviewed inventory of findings, high-complexity functions, and
measured clones. Edit decisions with concrete reason categories and evidence,
then use `review apply` to validate fingerprints and render the annotated
report. Missing items remain unreviewed; changed fingerprints are marked stale
with their prior decision visible. Keep the baseline report and ledger
together, and preserve the original baseline when auditing later revisions.
Ledger scope follows the requested paths and source categories. Reason labels
such as `cohesive-validation`, `lifecycle-ordering`, `distinct-semantics`,
`generated-contract`, `scenario-clarity`, `low-benefit`, and `upstream` still
need item-specific evidence; an upstream deferral also names its owner.
Ambiguous identities require a known, unchanged source manifest before a
decided ledger item can transfer; changed or unknown snapshots remain stale.
Structural counters are parser-only signals, and clone families group connected
duplicate regions while retaining each measured pair.

## GitHub Action

The root action builds the CLI source from its own `github.action_path` with
the Go version in `go.mod`, then runs `audit`. It does not download a release
artifact or invoke scripts from the target repository. Use the `v0.1.0` tag or
a reviewed commit when using it from another repository:

```yaml
- name: Check out the target
  uses: actions/checkout@v4

- name: Deburr audit
  id: deburr
  uses: kellen-miller/deburr@v0.1.0
  with:
    path: .
    format: github
```

Supported inputs are `path` (default `.`), `format` (`text`, `json`, `html`,
or `github`; default `github`), `duplicates` (default `false`), and `cpd`.
Set `duplicates: true` to opt into the same CPD 5.3.0 adapter exposed by the
CLI. The optional `cpd` value selects a caller-provided CPD 5.3.0 executable;
the action does not download or install an analyzer. It always writes to a
fresh, invocation-owned runner-temporary path outside the target tree. The
`report` output contains that path only when the CLI produced a report. The
action does not fail merely because findings exist; scanner errors preserve a
newly produced report and return the CLI status so an `always()` artifact step
can retain it:

```yaml
- name: Upload Deburr report
  if: always() && steps.deburr.outputs.report != ''
  uses: actions/upload-artifact@v4
  with:
    name: deburr-report
    path: ${{ steps.deburr.outputs.report }}
```

When duplication is enabled, provision CPD 5.3.0 in the caller workflow and
pass its executable path if it is not on `PATH`:

```yaml
- name: Deburr audit with duplication
  uses: kellen-miller/deburr@v0.1.0
  with:
    path: .
    format: json
    duplicates: true
    cpd: /opt/cpd-5.3.0/bin/cpd
```

The action rejects `cpd` unless `duplicates` is explicitly `true`, and it
rejects any CPD executable whose version is not 5.3.0. This keeps duplication
provisioning visible to the caller while the action remains a thin wrapper
around the CLI.

The composite action uses Bash. The repository workflow is configured to smoke
test the hosted Linux, macOS, and Windows runners. A self-hosted runner must
provide Bash and the runner capabilities required by `actions/setup-go`.

## Portable cleanup skill

[`skills/deburr/SKILL.md`](skills/deburr/SKILL.md) is the distributable,
harness-neutral entrypoint. It invokes `deburr guide cleanup` and keeps the
workflow in the embedded canonical guide instead of maintaining a second set
of cleanup instructions.

## Design direction

Deburr leads with findings, raw measurements, and changes over time. Existing
repository checks remain useful evidence and are not silently replaced. The
analyzer must make incomplete coverage visible, and cleanup must preserve
behavior and local ownership boundaries. Do not add helpers or abstractions
only to improve a measurement.

See [research notes](docs/research.md) and the
[Go analyzer/report contract](docs/design.md) for the current rationale and
schema direction.

## License

Deburr is available under the [MIT License](LICENSE).
