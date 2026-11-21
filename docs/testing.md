# Testing

Run `cargo test --locked` for the unit and integration tests. Nothing else
needs to be installed: clone detection is built in.

## Test layers

- Unit tests live beside the code in `src/` (analysis rules, identity,
  compare, review, renderers).
- `tests/*.rs` drive the library and the binary: `cli.rs` (exit codes and
  flags), `go_analysis.rs`, `go_findings.rs`, `typescript_analysis.rs`,
  `audit.rs`, `compare.rs`, `review.rs`, `render.rs`, and `duplication.rs`.
- `tests/corpus/` holds Go and TypeScript fixtures. `tests/corpus/dup` is a
  clean fixture with known clones; the Action and release smoke tests audit it.
  The corpus also contains deliberately malformed files, so `deburr audit .` at
  the repository root exits 1 by design.
- The duplication snapshot test pins the clone output over
  `tests/corpus/dup/snapshot`, so a `cpd-core` upgrade that changes results
  fails loudly. Renovate groups the `cpd-core` update and asks for this test.

Lint and formatting run in pull-request CI through
`kellen-miller/ci/.github/actions/rust-check`, which enforces a shared policy:
no `unwrap` or `expect` outside tests, documented `# Errors` and `# Panics`
sections, and limits on function length, cognitive complexity, and nesting. Fix
findings in code instead of suppressing them.

## Install script and Action

`scripts/test-install-deburr.sh` builds the checkout with a test version,
packages it as the release workflow does, and checks that
`scripts/install-deburr.sh` installs it with a good checksum and refuses a
corrupted checksum, a wrong version, and a plain-HTTP source. The pull-request
workflow runs it on Linux, macOS, and Windows, so the download path is
exercised before any release depends on it.

The Action tests in `.github/workflows/test.yml` run `uses: ./` on the three
operating systems. A local reference is not a release tag, so the Action builds
with `cargo build --release --locked`. The tests assert that duplication is
`measured` with clones, that `duplicates: false` yields `not_requested`, that a
large `min-clone-tokens` removes the clones, and that an invalid
`min-clone-lines` fails.

## Release dry run

Run the `Release` workflow manually (`workflow_dispatch`, `dry-run` left at
`true`). It builds and smoke-tests all five targets with a placeholder version
and skips the publish job, so no release or tag is created.

## Parity with the Go version

The Rust rewrite was checked against the retired Go implementation by reducing
both reports to a projection (files, categories, function identities, metrics,
signals, findings) and requiring no differences beyond the per-entry reasons in
`tests/parity/accepted.toml`. The Go-derived projections for `tests/corpus/`
are committed under `tests/corpus/expected/`, and
`rust_matches_committed_corpus_projections` checks them on every `cargo test`
without a Go toolchain.

IDs, fingerprints, condition text, float formatting, parse-error text, and HTML
bytes are not projected. Duplication is excluded because the engine changed.

To compare against the Go version again (for example after a grammar update),
build it from the last commit that contains it (the parent of the commit that
removed `go.mod`) and point the harness at it.
`DEBURR_BLESS_CORPUS=1` rewrites the committed projections from that binary;
review every change before committing it.

```sh
git worktree add /tmp/deburr-go-src "$(git log -1 --format=%H -- go.mod)^"
(cd /tmp/deburr-go-src && go build -o /tmp/deburr-go/deburr ./cmd/deburr)
DEBURR_GO_BIN=/tmp/deburr-go/deburr \
DEBURR_PARITY_ROOTS="$PWD:/path/to/another/project" \
  cargo test --locked --test parity -- --nocapture
```

## VS Code TypeScript integration

CI audits Microsoft's VS Code `src` tree at commit
`bd64649be19f2da0f09c2ab84c066c6c764b83be` without executing target code.
Reproduce locally:

```sh
git clone --filter=blob:none --no-checkout https://github.com/microsoft/vscode.git /tmp/deburr-vscode
git -C /tmp/deburr-vscode sparse-checkout set src
git -C /tmp/deburr-vscode checkout bd64649be19f2da0f09c2ab84c066c6c764b83be
DEBURR_VSCODE_SRC=/tmp/deburr-vscode/src cargo test --release --locked --test vscode -- --nocapture
```

The corpus contains 10,045 TypeScript files with zero parse errors and
270,201 functions, the same count the Go version recorded. The test prints the
count next to that baseline, so a difference means the grammar or function
discovery changed and needs an explanation. The full-projection comparison
with the Go binary differs only where the Go version's pure-Go parser was wrong
(generator-method names and method ranges stretched over a trailing comment);
`tests/parity/accepted.toml` lists those entries one by one. The vendored
TypeScript grammar carries the patches that make the whole corpus parse.

When changing the pinned corpus or a grammar, review every baseline change and
update the test and workflow together. Ordinary tests skip this large corpus
unless `DEBURR_VSCODE_SRC` is set.
