# deburr

Find the rough edges that accumulate as code evolves.

Deburr is a deterministic code-quality CLI for Go and TypeScript. It reports
function metrics, complexity, nesting, branch findings, and copy-pasted code
with source locations, so developers, coding agents, and CI can see where code
has grown hard to follow. Every result is tied to the same function
identities, so findings, complexity, and clones can be reviewed together. It
does not use models, rewrite files, or produce a composite quality score.

Deburr is a single Rust binary. Clone detection is built in; there is nothing
else to install.

**Status:** `1.0.0` rewrote Deburr in Rust. Reports use schema `3` and
analyzer `deburr` `1.0.0`; they are not comparable with reports from the Go
versions. See [Changes from the Go version](#changes-from-the-go-version).

## What it analyzes

- Go and TypeScript: `.go`, `.ts`, `.tsx`, `.mts`, `.cts`.
- Parser-only analysis with tree-sitter, including files excluded by build
  constraints. No type-checking, import resolution, or execution of target
  code.
- Tests are analyzed separately from production code. `vendor`, generated
  files, and `testdata` are excluded by default.
- Coverage reports what was analyzed, excluded, unsupported, or unparsed.
- JavaScript and framework component files are not supported.

Findings are advisory. Deburr exits nonzero for usage errors and for files it
cannot read or parse, not because findings exist.

## Install

Download a prebuilt binary from the
[releases page](https://github.com/kellen-miller/deburr/releases). Archives
exist for `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`,
`x86_64-apple-darwin`, `aarch64-apple-darwin`, and `x86_64-pc-windows-msvc`,
next to a `SHA256SUMS` file. The install script downloads over HTTPS, verifies
the checksum, and refuses to install on a mismatch:

```sh
version=1.0.0
target=aarch64-apple-darwin
curl --proto '=https' --fail --location --output install-deburr.sh \
  "https://raw.githubusercontent.com/kellen-miller/deburr/v$version/scripts/install-deburr.sh"
bash install-deburr.sh "$version" "$target" \
  "https://github.com/kellen-miller/deburr/releases/download/v$version" "$HOME/.local/bin"
```

To build from source, run `cargo build --release --locked` with a stable Rust
toolchain (1.97 or newer) and use `target/release/deburr`. A C compiler is
needed for the tree-sitter grammars.

## Quick start

```sh
# Print a text report
deburr audit .

# Write a JSON report outside the audited tree
deburr audit . --format json --output /tmp/deburr-before.json

# After changes, compare against the baseline
deburr audit . --format json --output /tmp/deburr-after.json
deburr compare /tmp/deburr-before.json /tmp/deburr-after.json
```

Reports can be rendered as `text` (default), `json`, `html`, or `github`.
`audit --output` rejects paths inside the audited directory, so keep reports
somewhere else. Audits print phase updates and file counts to stderr; stdout
stays report-only. Use `--quiet` to suppress progress. Run `deburr audit --help`
for all flags.

### Baselines and versions

Reports record the analyzer identity (`deburr` `1.0.0`, report schema `3`) and
the settings used, including the clone detection settings. `deburr --version`
prints the version of the binary. Changes to the analyzer version or to
measurement settings make reports incompatible, and `deburr compare` says so
instead of drawing a conclusion. Rerun the baseline with the same version and
settings before comparing. See [docs/design.md](docs/design.md) for details.

### Reviewing findings

`review init` creates a ledger to annotate with decisions and evidence;
`review apply` validates it and renders the annotated report:

```sh
deburr review init /tmp/deburr-before.json --output /tmp/deburr-review.json
deburr review apply /tmp/deburr-before.json --ledger /tmp/deburr-review.json \
  --format html --output /tmp/deburr-reviewed.html
```

## Duplication

Every audit looks for clones: repeated runs of code tokens in the files Deburr
analyzes. Production code and test code are searched as separate groups, so a
clone never pairs production code with a test, and each group gets its own
percentage. Nothing outside the analyzed files is read.

- `--min-clone-tokens N` (default 80) sets the shortest clone, in tokens.
- `--min-clone-lines N` (default 8) sets the shortest clone, in code lines. A
  clone is kept only when both of its regions cover at least `N` code lines;
  blank and comment lines do not count.
- `--no-duplicates` skips clone detection. The report then records duplication
  as `not_requested`, never as a zero measurement. `--duplicates` is accepted
  and has no effect.
- `.jscpd.json` is not read.

Each clone region lists the functions it overlaps by report ID, so clones,
findings, and complexity join on the same identities. The percentage is the
share of a category's code lines covered by any clone region (each line counted
once), so it never exceeds 100 and does not move when blank lines or comments
change. It differs from the figure a standalone jscpd run prints. Detection
uses the `cpd-core` library from [jscpd](https://github.com/kucherenko/jscpd)
on Deburr's own tree-sitter tokens.

## GitHub Action

At a release tag the action downloads the prebuilt binary for the runner and
verifies its checksum. At any other ref (a branch, a commit, or a local
`uses: ./`) it installs Rust and runs `cargo build --release --locked` on a
copy of the action's source outside your workspace, so your repository's
`.cargo/config.toml` and Cargo environment overrides (`build.target`,
`rustc-wrapper`, `target-dir`) cannot change the build.

```yaml
steps:
  - uses: actions/checkout@v4

  - id: deburr
    uses: kellen-miller/deburr@v1.0.0
    with:
      path: .
```

The report goes to a temp file outside the checkout, exposed as the `report`
output. To keep it:

```yaml
  - if: always() && steps.deburr.outputs.report != ''
    uses: actions/upload-artifact@v4
    with:
      name: deburr-report
      path: ${{ steps.deburr.outputs.report }}
```

| Input | Default | Description |
| --- | --- | --- |
| `path` | `.` | File or directory to audit |
| `format` | `github` | `text`, `json`, `html`, or `github` |
| `duplicates` | `true` | Detect clones; `false` skips detection |
| `min-clone-tokens` | `80` | Minimum tokens in a clone |
| `min-clone-lines` | `8` | Minimum code lines in each clone region |

The action needs neither Go, Node, nor a clone detector. The step fails on
analyzer errors, not on findings alone.

## Changes from the Go version

- Deburr is written in Rust and shipped as prebuilt binaries. `go install` no
  longer applies.
- Clone detection is built in and on by default. The flags `--cpd`, `--jscpd`,
  `--duplicates-config`, and `--enforce-duplicates-threshold`, and the matching
  Action inputs, are removed. Deburr no longer measures a threshold; it
  reports measurements.
- Reports use schema `3` and analyzer version `1.0.0`. `deburr compare`
  refuses to compare them with earlier reports.
- Findings compare branch bodies by code tokens rather than printed layout, so
  layout and comments never make two branches differ.

## Cleanup guide

`deburr guide cleanup` prints the workflow for acting on a report: keep a
baseline, make behavior-preserving changes, and compare. The portable
[skills/deburr/SKILL.md](skills/deburr/SKILL.md) skill invokes the same guide.

## Development

Run `cargo test --locked`. Pull-request CI runs `cargo fmt`, `cargo clippy`,
`cargo test`, and `cargo build` through
`kellen-miller/ci/.github/actions/rust-check` and its bundled lint policy; fix
findings in code rather than suppressing them. See
[docs/testing.md](docs/testing.md) for the test layers, the parity check
against the Go version, and the pinned VS Code integration test. Releases are
built by `.github/workflows/release.yaml` on pushes to `main`; a release is
published only after its assets are uploaded to a draft and verified (see
[docs/testing.md](docs/testing.md)).

## License

[MIT](LICENSE)
