---
name: deburr
description: Use when deterministic Deburr evidence is needed to review or clean up Go or TypeScript code while preserving behavior.
---

# Deburr

Use the `deburr` binary as the measurement boundary. Before acting, run
`deburr guide cleanup` and follow the canonical guide it prints. That command
is the source of truth for the cleanup loop; this portable entrypoint avoids a
second copy of those instructions.

If the binary is unavailable, install a release with
`scripts/install-deburr.sh VERSION TARGET SOURCE DEST` from the Deburr source
(see the README for the release URL), or set the working directory to the
checked-out Deburr source and run `cargo build --release --locked`; the binary
is `target/release/deburr`. The source checkout, not the target repository,
must be the build context. If neither is possible, report that an audit cannot
run. Do not substitute a different analyzer or claim that findings were
collected.

Before editing the target, read its instructions and inspect its current
status and diff. Preserve existing user changes. Keep baseline and iteration
reports in a harness-owned directory outside the scanned path, retain the
original baseline, and use unique output names. Run the target repository's
normal behavior checks after each justified change, then compare the new
report with the original baseline using matching analyzer and configuration
identities.

Audits include clone detection by default and need no other tools. Keep the
same `--min-clone-tokens` and `--min-clone-lines` (or `--no-duplicates`)
between baseline and later reports, since changing them makes reports
incompatible.

Treat findings, measurements, clones, and coverage as review evidence. Do not
add abstractions only to lower a measurement, apply automatic refactors, chase
zero findings, or impose a CI threshold that the caller did not request.
