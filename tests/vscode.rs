//! The pinned VS Code `src` checkout as real-world TypeScript.
//!
//! Skipped unless `DEBURR_VSCODE_SRC` points at the `src` directory of a
//! checkout of `bd64649be19f2da0f09c2ab84c066c6c764b83be`. Every one of its
//! 10,045 TypeScript files must parse with the patched grammar. The function
//! count is printed next to the count the Go version measured on the same
//! commit (270,201). The two matched exactly when this test was written, so
//! a difference now means the grammar or function discovery changed and
//! needs an explanation. The parity harness compares the full projections.

use std::env;
use std::io;
use std::path::Path;

use deburr::audit::{AuditOptions, audit};
use deburr::report::FileStatus;

const FILES: u32 = 10_045;
const GO_BASELINE_FUNCTIONS: usize = 270_201;

#[test]
fn every_vscode_file_parses_and_every_function_is_well_formed() {
    let Some(root) = env::var_os("DEBURR_VSCODE_SRC") else {
        eprintln!("vscode: skipped; set DEBURR_VSCODE_SRC to the pinned VS Code src checkout");

        return;
    };

    let options = AuditOptions {
        include_tests: true,
        include_testdata: false,
        include_vendor: false,
        include_generated: false,
        max_file_bytes: None,
        duplication: None,
    };
    let outcome = audit(Path::new(&root), &options, &mut io::sink());
    let report = &outcome.report;

    let failed: Vec<String> = report
        .files
        .iter()
        .filter(|file| matches!(file.status, FileStatus::ParseError | FileStatus::ReadError))
        .map(|file| format!("{}: {:?}", file.path, file.error))
        .take(10)
        .collect();

    assert!(failed.is_empty(), "files that did not parse: {failed:#?}");
    assert!(outcome.errors.is_empty(), "{:?}", outcome.errors);
    assert_eq!(report.coverage.read_errors, 0);
    assert_eq!(report.coverage.parse_errors, 0);
    assert_eq!(report.coverage.analyzed, FILES);

    for function in &report.functions {
        assert!(
            function.start.line >= 1
                && function.end.line >= function.start.line
                && function.cyclomatic >= 1
                && !function.fingerprint.is_empty()
                && function.id.starts_with("typescript/"),
            "invalid measurement: {function:?}"
        );
    }

    println!(
        "vscode: {} files analyzed, {} functions (Go baseline {GO_BASELINE_FUNCTIONS}, \
         difference {:+}), {} findings, {} parse errors",
        report.coverage.analyzed,
        report.functions.len(),
        i64::try_from(report.functions.len()).unwrap_or(i64::MAX)
            - i64::try_from(GO_BASELINE_FUNCTIONS).unwrap_or(i64::MAX),
        report.findings.len(),
        report.coverage.parse_errors,
    );
}
