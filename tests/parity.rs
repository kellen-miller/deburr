//! Semantic parity with the Go version, which has been removed from the tree.
//!
//! Two checks reduce reports to a projection (see `parity/projection.rs`) and
//! diff them, failing on any difference not listed in `parity/accepted.toml`:
//!
//! - `rust_matches_committed_corpus_projections` always runs. It audits each
//!   corpus root under `tests/corpus` with the Rust library and compares the
//!   result with the projection the Go version produced for that root, stored
//!   in `tests/corpus/expected/<root>.json`. No Go binary is needed.
//! - `rust_matches_go_on_parity_roots` runs only when `DEBURR_GO_BIN` (a
//!   `deburr` built from a pre-removal commit) and `DEBURR_PARITY_ROOTS` (a
//!   `:`-separated list of directories) are set. The Go binary audits each
//!   root to a temporary file, and its report is diffed with the Rust one.
//!
//! `bless_corpus_projections` rewrites the stored projections from a Go
//! binary when `DEBURR_GO_BIN` and `DEBURR_BLESS_CORPUS` are both set. Commit
//! the result only after reviewing it against the Go version's behavior.
//!
//! Nothing is ever written inside an audited root.

#[path = "parity/projection.rs"]
mod projection;

use std::env;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use deburr::audit::{AuditOptions, audit};
use projection::{AcceptedList, Projection, diff, project};

const ACCEPTED: &str = include_str!("parity/accepted.toml");

/// Corpus roots under `tests/corpus`. Each is audited on its own so the
/// stored projection files under `tests/corpus/expected` are never part of an
/// audited tree.
const CORPUS_ROOTS: &[&str] = &["dup", "go", "ts"];

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus")
}

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
    format!(
        "files {}, functions {}, findings {}",
        projection.len("files"),
        projection.len("functions"),
        projection.len("findings")
    )
}

/// Prints every difference between `go` and `rust` and returns how many are
/// not listed in the accepted divergences.
fn unlisted_differences(
    label: &str,
    go: &Projection,
    rust: &Projection,
    accepted: &AcceptedList,
) -> usize {
    let mut unlisted = 0_usize;
    let mut listed = 0_usize;

    for difference in diff(go, rust) {
        match accepted
            .accepted
            .iter()
            .find(|entry| entry.covers(&difference))
        {
            Some(entry) => {
                listed += 1;
                println!("parity: {label}: accepted {difference} ({})", entry.reason);
            }
            None => {
                unlisted += 1;
                println!("parity: {label}: {difference}");
            }
        }
    }

    println!(
        "parity: {label}: {unlisted} differences ({}, {listed} accepted)",
        summary(go)
    );

    unlisted
}

#[test]
fn rust_matches_committed_corpus_projections() {
    let accepted: AcceptedList = toml::from_str(ACCEPTED).expect("decode accepted.toml");
    let mut unlisted = 0_usize;

    for name in CORPUS_ROOTS {
        let root = corpus_dir().join(name);
        let stored_path = corpus_dir().join("expected").join(format!("{name}.json"));
        let stored = std::fs::read_to_string(&stored_path)
            .unwrap_or_else(|error| panic!("read {}: {error}", stored_path.display()));
        let stored: serde_json::Value = serde_json::from_str(&stored)
            .unwrap_or_else(|error| panic!("decode {}: {error}", stored_path.display()));
        let go = Projection::from_json(&stored)
            .unwrap_or_else(|error| panic!("load {}: {error}", stored_path.display()));
        let rust = project(&rust_report(&root));

        assert!(
            go.len("files") > 0 && go.len("functions") > 0,
            "{} holds no files or no functions; regenerate it",
            stored_path.display()
        );

        unlisted += unlisted_differences(&format!("corpus/{name}"), &go, &rust, &accepted);
    }

    assert_eq!(
        unlisted, 0,
        "corpus differs from the committed Go projections beyond accepted.toml"
    );
}

#[test]
fn rust_matches_go_on_parity_roots() {
    let (Some(go_bin), Some(roots)) = (
        env::var_os("DEBURR_GO_BIN"),
        env::var_os("DEBURR_PARITY_ROOTS"),
    ) else {
        eprintln!("parity: live Go comparison skipped; set DEBURR_GO_BIN and DEBURR_PARITY_ROOTS");

        return;
    };

    let accepted: AcceptedList = toml::from_str(ACCEPTED).expect("decode accepted.toml");
    let mut unlisted = 0_usize;

    for root in env::split_paths(&roots) {
        // A missing root makes both sides report nothing and "agree".
        assert!(
            root.is_dir(),
            "parity root {} is not a directory",
            root.display()
        );

        let go = project(&go_report(&go_bin, &root));
        let rust = project(&rust_report(&root));

        unlisted += unlisted_differences(&root.display().to_string(), &go, &rust, &accepted);
    }

    assert_eq!(
        unlisted, 0,
        "parity differences not listed in accepted.toml"
    );
}

#[test]
fn bless_corpus_projections() {
    let (Some(go_bin), Some(_)) = (
        env::var_os("DEBURR_GO_BIN"),
        env::var_os("DEBURR_BLESS_CORPUS"),
    ) else {
        eprintln!("parity: bless skipped; set DEBURR_GO_BIN and DEBURR_BLESS_CORPUS to rewrite");

        return;
    };

    let expected_dir = corpus_dir().join("expected");
    std::fs::create_dir_all(&expected_dir).expect("create expected dir");

    for name in CORPUS_ROOTS {
        let go = project(&go_report(&go_bin, &corpus_dir().join(name)));
        let mut text = serde_json::to_string_pretty(&go.to_json()).expect("encode projection");
        text.push('\n');

        std::fs::write(expected_dir.join(format!("{name}.json")), text)
            .expect("write stored projection");
        println!("parity: wrote expected/{name}.json ({})", summary(&go));
    }
}
