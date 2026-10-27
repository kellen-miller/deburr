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

The corpus contains 10,045 TypeScript files. Deburr analyzes all 10,045 and
records 270,201 functions with zero parse errors. The `gotreesitter` v0.56.0
update accepts the seven files previously rejected by v0.55.1, including
computed getters, generic import types, and contextual identifiers.

The integration test requires complete coverage and validates function
measurements. It fails on any parser error or changed corpus counts, so
future parser updates require reviewing changes to the baseline.

When changing the pinned corpus or parser, review every baseline change and
update the test and workflow together. Ordinary unit tests skip this large
corpus unless `DEBURR_VSCODE_SRC` is set.
