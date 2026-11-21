//! Semantic parity between the Go reference binary and the Rust library.
//!
//! Skipped unless `DEBURR_GO_BIN` (a built Go `deburr`) and
//! `DEBURR_PARITY_ROOTS` (a `:`-separated list of directories) are set. For
//! each root the Go binary writes a JSON report to a temporary file, the Rust
//! library audits the same root in memory, both are reduced to a projection,
//! and any difference not listed in `parity/accepted.toml` fails the test.
//! Nothing is ever written inside a root.

#[path = "parity/projection.rs"]
mod projection;

use std::env;
use std::ffi::OsString;
use std::io;
use std::path::Path;
use std::process::Command;

use deburr::audit::{AuditOptions, audit};
use projection::{AcceptedList, Difference, Projection, diff, project};

const ACCEPTED: &str = include_str!("parity/accepted.toml");

fn go_report(go_bin: &OsString, root: &Path) -> serde_json::Value {
    let dir = tempfile::tempdir().expect("create temp dir");
    let output = dir.path().join("go-report.json");

    // The Go binary exits 1 for reports with parse errors but still writes
    // them, so only a missing report is a failure.
    let status = Command::new(go_bin)
        .arg("audit")
        .arg(root)
        .args(["--format", "json", "--quiet", "--output"])
        .arg(&output)
        .status()
        .expect("run the Go deburr binary");
    let text = std::fs::read_to_string(&output)
        .unwrap_or_else(|error| panic!("Go deburr wrote no report (exit {status}): {error}"));

    serde_json::from_str(&text).expect("decode Go report")
}

fn rust_report(root: &Path) -> serde_json::Value {
    let options = AuditOptions {
        include_tests: true,
        include_testdata: false,
        include_vendor: false,
        include_generated: false,
        max_file_bytes: None,
        duplication: None,
    };
    let outcome = audit(root, &options, &mut io::sink());

    serde_json::to_value(&outcome.report).expect("encode Rust report")
}

fn summary(projection: &Projection) -> String {
    format!("files {}", projection.len("files"))
}

#[test]
fn rust_matches_go_on_parity_roots() {
    let (Some(go_bin), Some(roots)) = (
        env::var_os("DEBURR_GO_BIN"),
        env::var_os("DEBURR_PARITY_ROOTS"),
    ) else {
        eprintln!("parity: skipped; set DEBURR_GO_BIN and DEBURR_PARITY_ROOTS to run");

        return;
    };

    let accepted: AcceptedList = toml::from_str(ACCEPTED).expect("decode accepted.toml");
    let mut unlisted = 0_usize;

    for root in env::split_paths(&roots) {
        let go = project(&go_report(&go_bin, &root));
        let rust = project(&rust_report(&root));
        let differences = diff(&go, &rust);

        let (listed, remaining): (Vec<Difference>, Vec<Difference>) =
            differences.into_iter().partition(|difference| {
                accepted
                    .accepted
                    .iter()
                    .any(|entry| entry.covers(difference))
            });

        for difference in &listed {
            let reason = accepted
                .accepted
                .iter()
                .find(|entry| entry.covers(difference))
                .map_or("", |entry| entry.reason.as_str());

            println!(
                "parity: {}: accepted {difference} ({reason})",
                root.display()
            );
        }

        for difference in &remaining {
            println!("parity: {}: {difference}", root.display());
        }

        println!(
            "parity: {}: {} differences ({}, {} accepted)",
            root.display(),
            remaining.len(),
            summary(&go),
            listed.len()
        );
        unlisted += remaining.len();
    }

    assert_eq!(
        unlisted, 0,
        "parity differences not listed in accepted.toml"
    );
}
