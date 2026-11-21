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

The corrupted-checksum case flips the first hex digit of the digest to a
different digit and asserts the file changed before running the negative test,
so it cannot silently become a no-op.

`action-hostile-config-tests` plants a `.cargo/config.toml` in the workspace
(`build.target` set to a bogus triple, a `build.rustc-wrapper` script that
fails, and a `build.target-dir`) together with the matching `RUSTC_WRAPPER`,
`CARGO_BUILD_TARGET`, and `CARGO_TARGET_DIR` environment variables, then runs
`uses: ./` and requires a measured audit and no `target` or `hostile-target`
directory in the workspace. The Action builds a copy of its own source under
`$RUNNER_TEMP`, outside the workspace and any `.cargo` directory, with an
explicit `--target-dir` and the wrapper, target, target-dir, and rustflags
environment overrides cleared, so the audited repository's Cargo configuration
cannot redirect the build or run its code.

## Release flow

`.github/workflows/release.yaml` never exposes a tag or a published release
without verified assets:

1. `version` runs the shared release action with `dry-run: 'true'`, which only
   computes the next version.
2. `build` compiles and smoke-tests the five targets and uploads the archives
   as workflow artifacts.
3. One `release` job writes `SHA256SUMS` and runs `scripts/install-deburr.sh`
   against the local directory. This step also runs in the manual dry run.
4. On a real release it creates a draft with
   `gh release create v<version> --draft --target <sha>` (GitHub creates the tag
   only when the draft is published), uploads every asset, downloads them back
   with `gh release download`, verifies the checksums and the install script
   against that copy, and only then runs `gh release edit --draft=false`.
5. Any failure or cancellation before publishing deletes the draft.
6. After publishing, the job runs `scripts/install-deburr.sh` against the real
   HTTPS download URL (with a few retries for CDN lag). If that fails, the job
   deletes the release together with its tag (only when the tag points at this
   run's commit) and fails, so no tag is left without downloadable assets and a
   re-run starts clean.

An existing tag or published release for the version stops the job before
anything is created; a leftover draft from an interrupted run is replaced.

## Release dry run

Run the `Release` workflow manually (`workflow_dispatch`, `dry-run` left at
`true`). It builds and smoke-tests all five targets with a placeholder version,
assembles `SHA256SUMS`, and checks the install script against the archives. It
creates no draft, release, or tag.

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
`tests/parity/accepted.toml` lists those entries one by one. The forked
TypeScript grammar carries the patches that make the whole corpus parse.

When changing the pinned corpus or a grammar, review every baseline change and
update the test and workflow together. Ordinary tests skip this large corpus
unless `DEBURR_VSCODE_SRC` is set.
