# deburr

Find the rough edges that accumulate as code evolves.

Deburr is a deterministic code-quality CLI for Go and TypeScript. It reports
function metrics, complexity, nesting, and branch findings with source
locations, so developers, coding agents, and CI can see where code has grown
hard to follow. It does not use models, rewrite files, or produce a composite
quality score.

**Status:** `v0.2.0` is experimental and pre-1.0; reports and flags may change.

## What it analyzes

- Go and TypeScript: `.go`, `.ts`, `.tsx`, `.mts`, `.cts`.
- Parser-only analysis, including files excluded by build constraints. No
  type-checking, import resolution, or execution of target code.
- Tests are analyzed separately from production code. `vendor`, generated
  files, and `testdata` are excluded by default.
- Coverage reports what was analyzed, excluded, unsupported, or unparsed.
- JavaScript and framework component files are not supported.

Findings are advisory. Deburr exits nonzero for usage errors and for files it
cannot read or parse, not because findings exist.

## Install

```sh
go install github.com/kellen-miller/deburr/cmd/deburr@v0.2.0
```

Installing or building from source requires the Go version in `go.mod`. The
compiled binary needs neither Go nor Node for native audits.

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

Reports record the analyzer identity (currently `deburr/native` `0.3.0`,
report schema `2`) and the settings used. These differ from the release tag;
`deburr --version` prints the analyzer version. Changes to analyzer or detector
versions or measurement settings can make reports incompatible. Rerun the
baseline with matching versions and settings before comparing. See
[docs/design.md](docs/design.md) for details.

### Reviewing findings

`review init` creates a ledger to annotate with decisions and evidence;
`review apply` validates it and renders the annotated report:

```sh
deburr review init /tmp/deburr-before.json --output /tmp/deburr-review.json
deburr review apply /tmp/deburr-before.json --ledger /tmp/deburr-review.json \
  --format html --output /tmp/deburr-reviewed.html
```

## Duplication (optional)

Duplication detection is off by default and uses an external detector you
install yourself. Deburr supports any CPD/jscpd release in major version 5:

```sh
npm install -g jscpd@5
deburr audit . --duplicates
```

`--duplicates` uses `cpd`, then `jscpd`, from `PATH`; `--cpd PATH` or
`--jscpd PATH` selects an executable.

- The nearest `.jscpd.json` up to the Git root is used; override it with
  `--duplicates-config FILE`. Config paths and globs are relative to the config
  directory, and `.gitignore` rules apply unless `gitignore: false` is set.
- Config scopes only narrow duplication. Production and test code are measured
  separately, so percentages can differ from a standalone jscpd run.
- A configured threshold is advisory unless `--enforce-duplicates-threshold`
  is passed. Exceeded thresholds still keep the clone evidence.
- Empty scopes, unsupported settings, and `skipLocal: true` are errors.

Reports record the detector version and effective config. See
[docs/design.md](docs/design.md) for the supported settings.

## GitHub Action

The action sets up Go, builds Deburr from its own source, and runs `audit`:

```yaml
steps:
  - uses: actions/checkout@v4

  - id: deburr
    uses: kellen-miller/deburr@v0.2.0
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
| `duplicates` | `false` | Enable duplication detection |
| `cpd` / `jscpd` | | Detector executable (use one) |
| `duplicates-config` | | Explicit `.jscpd.json` |
| `enforce-duplicates-threshold` | `false` | Fail when the threshold is exceeded |

Duplication inputs require `duplicates: true`. The action does not install
CPD or jscpd, so provision it first:

```yaml
  - uses: actions/setup-node@v4
  - run: npm install -g jscpd@5
  - uses: kellen-miller/deburr@v0.2.0
    with:
      duplicates: true
```

The step fails on analyzer errors or an enforced duplication threshold, not on
findings alone.

## Cleanup guide

`deburr guide cleanup` prints the workflow for acting on a report: keep a
baseline, make behavior-preserving changes, and compare. The portable
[skills/deburr/SKILL.md](skills/deburr/SKILL.md) skill invokes the same guide.

## Development

Run `go test ./...`. See [docs/testing.md](docs/testing.md) for the pinned
VS Code integration test and known TypeScript parser limitations.

## License

[MIT](LICENSE)
