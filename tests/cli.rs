//! Exit codes and top-level behavior of the `deburr` binary.

use std::fs;
use std::process::{Command, Output};

fn deburr(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_deburr"))
        .args(args)
        .output()
        .expect("run deburr")
}

fn code(output: &Output) -> i32 {
    output.status.code().expect("exit code")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn no_arguments_is_a_usage_error() {
    assert_eq!(code(&deburr(&[])), 2);
}

#[test]
fn unknown_command_is_a_usage_error() {
    assert_eq!(code(&deburr(&["frobnicate"])), 2);
}

#[test]
fn unknown_flag_is_a_usage_error() {
    assert_eq!(code(&deburr(&["audit", ".", "--frobnicate"])), 2);
    assert_eq!(code(&deburr(&["--frobnicate"])), 2);
}

#[test]
fn help_exits_zero_and_lists_commands() {
    let output = deburr(&["--help"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_eq!(code(&output), 0);

    for command in ["audit", "compare", "review", "guide"] {
        assert!(stdout.contains(command), "help should mention {command}");
    }
}

#[test]
fn audit_help_lists_duplication_flags() {
    let output = deburr(&["audit", "--help"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_eq!(code(&output), 0);

    for flag in [
        "--no-duplicates",
        "--min-clone-tokens",
        "--min-clone-lines",
        "--duplicates",
    ] {
        assert!(stdout.contains(flag), "audit help should mention {flag}");
    }
}

#[test]
fn version_prints_name_and_version() {
    let output = deburr(&["--version"]);
    let want = format!("deburr {}\n", deburr_version());

    assert_eq!(code(&output), 0);
    assert_eq!(String::from_utf8_lossy(&output.stdout), want);
}

fn deburr_version() -> &'static str {
    option_env!("DEBURR_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
}

#[test]
fn audit_needs_exactly_one_path() {
    assert_eq!(code(&deburr(&["audit"])), 2);
    assert_eq!(code(&deburr(&["audit", "a", "b"])), 2);
}

#[test]
fn audit_rejects_bad_flag_values() {
    assert_eq!(code(&deburr(&["audit", ".", "--format", "yaml"])), 2);
    assert_eq!(code(&deburr(&["audit", ".", "--max-file-bytes", "0"])), 2);
    assert_eq!(code(&deburr(&["audit", ".", "--min-clone-tokens", "0"])), 2);
}

#[test]
fn audit_output_inside_audited_tree_is_rejected() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("a.go"), "package a\n").expect("write source");

    let tree = dir.path().to_str().expect("utf-8 path");
    let inside = dir.path().join("report.json");
    let output = deburr(&[
        "audit",
        tree,
        "--format",
        "json",
        "--output",
        inside.to_str().expect("utf-8 path"),
    ]);

    assert_eq!(code(&output), 2);
    assert!(
        !inside.exists(),
        "no report may be written inside the audited tree"
    );
}

#[test]
fn audit_output_dash_is_rejected() {
    assert_eq!(code(&deburr(&["audit", ".", "--output", "-"])), 2);
}

#[test]
fn audit_output_outside_tree_is_accepted_by_validation() {
    let tree = tempfile::tempdir().expect("tempdir");
    let elsewhere = tempfile::tempdir().expect("tempdir");
    let target = elsewhere.path().join("report.json");
    let output = deburr(&[
        "audit",
        tree.path().to_str().expect("utf-8 path"),
        "--format",
        "json",
        "--quiet",
        "--output",
        target.to_str().expect("utf-8 path"),
    ]);

    // Not rejected as a usage error: the report is written to the target.
    assert_eq!(code(&output), 0, "stderr: {}", stderr(&output));
    assert!(target.is_file(), "report must be written outside the tree");

    let leftovers: Vec<_> = fs::read_dir(elsewhere.path())
        .expect("read dir")
        .map(|entry| entry.expect("entry").file_name())
        .collect();
    assert!(
        leftovers
            .iter()
            .all(|name| !name.to_string_lossy().starts_with(".deburr-report-")),
        "temp files must be cleaned up: {leftovers:?}"
    );
}

#[test]
fn audit_writes_json_report_to_stdout() {
    let tree = tempfile::tempdir().expect("tempdir");
    let output = deburr(&[
        "audit",
        tree.path().to_str().expect("utf-8 path"),
        "--format",
        "json",
        "--quiet",
    ]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_eq!(code(&output), 0, "stderr: {}", stderr(&output));
    assert!(stdout.ends_with("}\n"));
    assert!(stdout.contains("\"schema_version\": \"3\""));
}

#[test]
fn guide_requires_the_cleanup_subcommand() {
    assert_eq!(code(&deburr(&["guide"])), 2);
    assert_eq!(code(&deburr(&["guide", "other"])), 2);
}

#[test]
fn compare_needs_two_reports() {
    assert_eq!(code(&deburr(&["compare", "only-one.json"])), 2);
}

#[test]
fn review_needs_a_subcommand() {
    assert_eq!(code(&deburr(&["review"])), 2);
    assert_eq!(code(&deburr(&["review", "apply", "report.json"])), 2);
}
