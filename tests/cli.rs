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

    for command in ["audit", "compare", "render", "review", "guide"] {
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
    assert!(stdout.contains("\"schema_version\": \"4\""));
}

/// Audits a one-file Go tree to JSON; returns the tree and the report path.
fn audited_report(output_dir: &tempfile::TempDir) -> (tempfile::TempDir, std::path::PathBuf) {
    let tree = tempfile::tempdir().expect("tempdir");
    fs::write(
        tree.path().join("a.go"),
        "package a\n\nfunc One() int {\n\treturn 1\n}\n",
    )
    .expect("write source");

    let report = output_dir.path().join("report.json");
    let output = deburr(&[
        "audit",
        tree.path().to_str().expect("utf-8 path"),
        "--format",
        "json",
        "--quiet",
        "--output",
        report.to_str().expect("utf-8 path"),
    ]);

    assert_eq!(code(&output), 0, "stderr: {}", stderr(&output));

    (tree, report)
}

#[test]
fn render_round_trips_an_audit_report_into_every_format() {
    let elsewhere = tempfile::tempdir().expect("tempdir");
    let (tree, report) = audited_report(&elsewhere);
    let report_arg = report.to_str().expect("utf-8 path");

    for format in ["text", "markdown", "html", "github"] {
        let output = deburr(&["render", report_arg, "--format", format]);

        assert_eq!(code(&output), 0, "{format}: {}", stderr(&output));
        assert!(
            !output.stdout.is_empty(),
            "{format} rendered nothing: {}",
            stderr(&output)
        );
    }

    let json = deburr(&["render", report_arg, "--format", "json"]);
    assert_eq!(code(&json), 0, "stderr: {}", stderr(&json));
    assert_eq!(
        String::from_utf8_lossy(&json.stdout),
        fs::read_to_string(&report).expect("read report"),
        "json render must reproduce the audit's JSON"
    );

    let rendered = elsewhere.path().join("out.txt");
    let written = deburr(&[
        "render",
        report_arg,
        "--source",
        tree.path().to_str().expect("utf-8 path"),
        "--output",
        rendered.to_str().expect("utf-8 path"),
    ]);

    assert_eq!(code(&written), 0, "stderr: {}", stderr(&written));
    assert!(rendered.is_file(), "--output must write the file");
    assert!(
        !String::from_utf8_lossy(&fs::read(&rendered).expect("read output")).contains('\u{1b}'),
        "output files never contain color codes"
    );
}

#[test]
fn render_validates_output_like_the_other_commands() {
    let elsewhere = tempfile::tempdir().expect("tempdir");
    let (_tree, report) = audited_report(&elsewhere);
    let report_arg = report.to_str().expect("utf-8 path");

    assert_eq!(code(&deburr(&["render", report_arg, "--output", "-"])), 2);
    assert_eq!(
        code(&deburr(&["render", report_arg, "--output", report_arg])),
        2,
        "output may not overwrite the input report"
    );
    assert_eq!(
        code(&deburr(&[
            "render",
            report_arg,
            "--output",
            elsewhere.path().to_str().expect("utf-8 path")
        ])),
        2,
        "output may not be a directory"
    );
}

#[test]
fn render_rejects_bad_input() {
    assert_eq!(code(&deburr(&["render"])), 2);
    assert_eq!(
        code(&deburr(&["render", "report.json", "--format", "yaml"])),
        2
    );
    assert_eq!(code(&deburr(&["render", "/no/such/report.json"])), 1);
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

#[test]
fn render_ignores_a_source_that_does_not_match_the_report() {
    let elsewhere = tempfile::tempdir().expect("tempdir");
    let (tree, report) = audited_report(&elsewhere);
    let output = deburr(&[
        "render",
        report.to_str().expect("utf-8 path"),
        "--format",
        "html",
        "--source",
        tree.path().to_str().expect("utf-8 path"),
    ]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_eq!(code(&output), 0, "stderr: {}", stderr(&output));
    assert!(!stdout.contains("return 1"), "snippet leaked into output");
    assert!(stderr(&output).contains("showing locations only"));
}
