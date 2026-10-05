# Testing

Run `go test ./...` for unit tests. Install `cpd@5` and `jscpd@5` on PATH to
include the real detector integration tests.

## VS Code TypeScript integration

CI audits Microsoft's VS Code `src` tree at commit
`bd64649be19f2da0f09c2ab84c066c6c764b83be`, without CGO or executing target
code. Reproduce locally:

```sh
git clone --filter=blob:none --no-checkout https://github.com/microsoft/vscode.git /tmp/deburr-vscode
git -C /tmp/deburr-vscode sparse-checkout set src
git -C /tmp/deburr-vscode checkout bd64649be19f2da0f09c2ab84c066c6c764b83be
DEBURR_VSCODE_SRC=/tmp/deburr-vscode/src CGO_ENABLED=0 \
  go test -v ./internal/analysis -run '^TestVSCodeTypeScriptIntegration$' -count=1 -timeout=15m
```

The corpus contains 10,045 TypeScript files. Deburr analyzes 10,038 and records
269,947 functions. Seven valid files fail the upstream `gotreesitter` v0.55.1
parser: computed getters, generic import types, and contextual identifiers
(`unique` and tuple label `symbol`). VS Code's TypeScript compiler accepts
these files with zero syntax diagnostics. Newer upstream code at
`343e2113176a6681afc1228c3b6a057348d40baa` also rejects these constructs.

The integration test lists the seven paths explicitly. It checks coverage,
function measurements, and that rejected files produce no partial metrics.
It fails on new parser errors or changed coverage. It also fails when a known
failure disappears, requiring removal from the baseline. A passing test means
this baseline is unchanged; it does **not** mean all VS Code files parse.

When changing the pinned corpus or parser, review every baseline change and
update the test and workflow together. Ordinary unit tests skip this large
corpus unless `DEBURR_VSCODE_SRC` is set.
