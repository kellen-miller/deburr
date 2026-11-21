//! Built-in clone detection: what a clone reports, which functions it names,
//! how the line minimum and percentages behave, and a snapshot that pins the
//! detector's output so a `cpd-core` upgrade that changes results fails here.

use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;

use deburr::audit::{AuditOptions, audit};
use deburr::duplication::DuplicationSettings;
use deburr::report::{Category, CodeClone, DuplicationStatus, Report};

mod ownership;
mod snapshot;

const PERCENT_TOLERANCE: f64 = 1e-9;

fn settings(min_tokens: usize, min_lines: usize) -> Option<DuplicationSettings> {
    Some(DuplicationSettings {
        min_tokens,
        min_lines,
    })
}

fn run(root: &Path, duplication: Option<DuplicationSettings>) -> Report {
    let options = AuditOptions {
        include_tests: true,
        include_testdata: false,
        include_vendor: false,
        include_generated: false,
        max_file_bytes: None,
        duplication,
    };

    audit(root, &options, &mut io::sink()).report
}

/// The shipped defaults: 80 tokens and 8 code lines.
fn run_default(root: &Path) -> Report {
    run(root, settings(80, 8))
}

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent directories");
    }

    fs::write(path, contents).expect("write fixture");
}

/// A Go function of `statements + 4` lines and about `7 * statements` tokens.
fn go_function(name: &str, statements: usize) -> String {
    let mut text = format!("func {name}(x int) int {{\n\ttotal := 0\n");

    for index in 1..=statements {
        text.push_str(&format!("\ttotal += x * {index}\n"));
    }

    text.push_str("\treturn total\n}\n");

    text
}

/// A Go file whose every line is code: `package a` and `lines - 1` variables.
/// With `noise`, each declaration is followed by a blank and a comment line,
/// which change the layout but not the tokens.
fn go_variables(lines: usize, noise: bool) -> String {
    let mut text = String::from("package a\n");

    for index in 1..lines {
        text.push_str(&format!("var v{index} = {index}\n"));

        if noise {
            text.push_str("\n// a note that is not code\n");
        }
    }

    text
}

fn production(report: &Report) -> &deburr::report::DuplicationCategory {
    report
        .duplication
        .categories
        .iter()
        .find(|row| row.category == Category::Production)
        .expect("production measurement")
}

/// Every function a clone endpoint names is a function of the same report.
fn assert_functions_resolve(report: &Report) {
    for clone in &report.duplication.clones {
        for endpoint in &clone.endpoints {
            for id in &endpoint.functions {
                assert!(
                    report.functions.iter().any(|function| &function.id == id),
                    "clone {} names unknown function {id}",
                    clone.id
                );
            }
        }
    }
}

fn endpoint_paths(clone: &CodeClone) -> Vec<&str> {
    let mut paths: Vec<&str> = clone
        .endpoints
        .iter()
        .map(|endpoint| endpoint.path.as_str())
        .collect();
    paths.sort_unstable();

    paths
}

#[test]
fn test_files_never_pair_with_production_files() {
    let dir = tempfile::tempdir().expect("temp dir");
    let source = format!("package a\n\n{}", go_function("Alpha", 20));
    write(dir.path(), "a.go", &source);
    write(dir.path(), "a_test.go", &source);

    let report = run_default(dir.path());

    assert_eq!(report.duplication.status, DuplicationStatus::Measured);
    assert!(report.duplication.clones.is_empty());
    assert_eq!(report.duplication.categories.len(), 2);

    for row in &report.duplication.categories {
        assert_eq!(row.files, 1, "{:?}", row.category);
        assert_eq!(row.duplicated_code_lines, 0, "{:?}", row.category);
    }
}

#[test]
fn tests_are_measured_in_their_own_group() {
    let dir = tempfile::tempdir().expect("temp dir");
    let function = go_function("Alpha", 20);
    write(dir.path(), "a_test.go", &format!("package a\n\n{function}"));
    write(dir.path(), "b_test.go", &format!("package b\n\n{function}"));

    let report = run_default(dir.path());

    assert_eq!(report.duplication.clones.len(), 1);
    assert_eq!(report.duplication.clones[0].category, Category::Test);
    assert_eq!(production(&report).files, 0);
}

#[test]
fn no_duplicates_leaves_duplication_not_requested() {
    let dir = tempfile::tempdir().expect("temp dir");
    let function = go_function("Alpha", 20);
    write(dir.path(), "a.go", &format!("package a\n\n{function}"));
    write(dir.path(), "b.go", &format!("package b\n\n{function}"));

    let report = run(dir.path(), None);

    assert_eq!(report.duplication.status, DuplicationStatus::NotRequested);
    assert!(report.duplication.clones.is_empty());
    assert!(report.duplication.categories.is_empty());
    assert!(!report.config.duplication.requested);
}

/// How many code lines two copies cover, and whether a clone is expected.
struct LineCase {
    code_lines: usize,
    clone_expected: bool,
}

/// Which of the two copies is padded with blank and comment lines.
struct Padding {
    in_a: bool,
    in_b: bool,
}

#[test]
fn the_line_minimum_counts_code_lines_on_both_endpoints() {
    let cases = [
        LineCase {
            code_lines: 7,
            clone_expected: false,
        },
        LineCase {
            code_lines: 8,
            clone_expected: true,
        },
        LineCase {
            code_lines: 9,
            clone_expected: true,
        },
    ];

    // Which copy is padded with blank and comment lines, and which file it
    // lands in, must not matter.
    let paddings = [
        Padding {
            in_a: false,
            in_b: false,
        },
        Padding {
            in_a: false,
            in_b: true,
        },
        Padding {
            in_a: true,
            in_b: false,
        },
    ];

    for case in &cases {
        for padding in &paddings {
            let dir = tempfile::tempdir().expect("temp dir");
            write(
                dir.path(),
                "a.go",
                &go_variables(case.code_lines, padding.in_a),
            );
            write(
                dir.path(),
                "b.go",
                &go_variables(case.code_lines, padding.in_b),
            );

            let report = run(dir.path(), settings(5, 8));
            let label = format!(
                "{} lines, padding a={} b={}",
                case.code_lines, padding.in_a, padding.in_b
            );

            assert_eq!(
                !report.duplication.clones.is_empty(),
                case.clone_expected,
                "{label}"
            );

            for clone in &report.duplication.clones {
                for endpoint in &clone.endpoints {
                    assert_eq!(endpoint.code_lines as usize, case.code_lines, "{label}");
                }
            }
        }
    }
}

#[test]
fn blank_lines_and_comments_change_neither_clones_nor_percentage() {
    let plain = tempfile::tempdir().expect("temp dir");
    let function = go_function("Alpha", 20);
    write(plain.path(), "a.go", &format!("package a\n\n{function}"));
    write(plain.path(), "b.go", &format!("package b\n\n{function}"));

    let padded = tempfile::tempdir().expect("temp dir");
    let spaced = function.replace("\n\ttotal +=", "\n\n\t// step\n\ttotal +=");
    write(padded.path(), "a.go", &format!("package a\n\n{function}"));
    write(
        padded.path(),
        "b.go",
        &format!("// header\n\npackage b\n\n{spaced}\n// trailer\n"),
    );

    let before = run_default(plain.path());
    let after = run_default(padded.path());
    let ids = |report: &Report| -> Vec<String> {
        report
            .duplication
            .clones
            .iter()
            .map(|clone| format!("{} {} {}", clone.id, clone.family_id, clone.tokens))
            .collect()
    };

    assert_eq!(before.duplication.clones.len(), 1);
    assert_eq!(ids(&before), ids(&after));
    assert!(
        (production(&before).percentage - production(&after).percentage).abs() < PERCENT_TOLERANCE
    );
    assert_eq!(
        production(&before).duplicated_code_lines,
        production(&after).duplicated_code_lines
    );
}

#[test]
fn percentages_are_bounded_by_one_hundred() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut source = String::from("package a\n\n");

    for name in ["One", "Two", "Three", "Four"] {
        source.push_str(&go_function(name, 12));
        source.push('\n');
    }

    // Four near-copies in one file and an identical second file overlap in
    // many ways; each line is still counted once.
    write(dir.path(), "a.go", &source);
    write(dir.path(), "b.go", &source);

    let report = run_default(dir.path());

    assert!(!report.duplication.clones.is_empty());

    for row in &report.duplication.categories {
        assert!(row.percentage <= 100.0, "{row:?}");
        assert!(row.duplicated_code_lines <= row.code_lines, "{row:?}");
    }

    let identical = tempfile::tempdir().expect("temp dir");
    write(identical.path(), "a.go", &go_variables(30, false));
    write(identical.path(), "b.go", &go_variables(30, false));

    let whole = run_default(identical.path());

    assert!((production(&whole).percentage - 100.0).abs() < PERCENT_TOLERANCE);
    assert_eq!(production(&whole).duplicated_code_lines, 60);
}

#[test]
fn moving_a_clone_down_keeps_its_clone_id() {
    let function = go_function("Alpha", 20);
    let before_dir = tempfile::tempdir().expect("temp dir");
    write(
        before_dir.path(),
        "a.go",
        &format!("package a\n\n{function}"),
    );
    write(
        before_dir.path(),
        "b.go",
        &format!("package b\n\n{function}"),
    );

    let after_dir = tempfile::tempdir().expect("temp dir");
    let shift = "// shifted\n".repeat(10);
    write(
        after_dir.path(),
        "a.go",
        &format!("package a\n\n{function}"),
    );
    write(
        after_dir.path(),
        "b.go",
        &format!("package b\n\n{shift}{function}"),
    );

    let before = run_default(before_dir.path());
    let after = run_default(after_dir.path());
    let (old, new) = (&before.duplication.clones[0], &after.duplication.clones[0]);
    let start_of = |clone: &CodeClone, path: &str| {
        clone
            .endpoints
            .iter()
            .find(|endpoint| endpoint.path == path)
            .map(|endpoint| endpoint.start.line)
    };

    assert_eq!(old.id, new.id);
    assert_eq!(old.family_id, new.family_id);
    assert_eq!(old.fingerprint, new.fingerprint);
    assert_eq!(start_of(new, "a.go"), start_of(old, "a.go"));
    assert_eq!(
        start_of(new, "b.go"),
        start_of(old, "b.go").map(|line| line + 10)
    );
}

#[test]
fn editing_inside_one_copy_replaces_the_clone() {
    let function = go_function("Alpha", 20);
    let edited = function.replace("x * 7", "x * 70");
    let before_dir = tempfile::tempdir().expect("temp dir");
    write(
        before_dir.path(),
        "a.go",
        &format!("package a\n\n{function}"),
    );
    write(
        before_dir.path(),
        "b.go",
        &format!("package b\n\n{function}"),
    );

    let after_dir = tempfile::tempdir().expect("temp dir");
    write(
        after_dir.path(),
        "a.go",
        &format!("package a\n\n{function}"),
    );
    write(after_dir.path(), "b.go", &format!("package b\n\n{edited}"));

    let before = run_default(before_dir.path());
    let after = run_default(after_dir.path());

    // The edit splits the one clone around the changed token (or shrinks it
    // below the minimum); no piece is the old clone.
    assert_eq!(before.duplication.clones.len(), 1);
    assert!(
        after
            .duplication
            .clones
            .iter()
            .all(|clone| clone.id != before.duplication.clones[0].id)
    );
}

#[test]
fn a_terminator_edit_changes_the_function_and_breaks_a_branch_match() {
    let joined = "package a\n\nfunc f(c bool) {\n\tif c {\n\t\treturn g()\n\t} else {\n\t\treturn g()\n\t}\n}\n";
    let split = "package a\n\nfunc f(c bool) {\n\tif c {\n\t\treturn g()\n\t} else {\n\t\treturn\n\t\tg()\n\t}\n}\n";
    let joined_dir = tempfile::tempdir().expect("temp dir");
    let split_dir = tempfile::tempdir().expect("temp dir");
    write(joined_dir.path(), "a.go", joined);
    write(split_dir.path(), "a.go", split);

    let joined_report = run_default(joined_dir.path());
    let split_report = run_default(split_dir.path());

    assert_eq!(joined_report.findings.len(), 1);
    assert_eq!(joined_report.findings[0].rule, "duplicate-branches");
    assert!(split_report.findings.is_empty());
    assert_eq!(joined_report.functions[0].id, split_report.functions[0].id);
    assert_ne!(
        joined_report.functions[0].fingerprint,
        split_report.functions[0].fingerprint
    );
}

fn deburr(args: &[&str]) -> serde_json::Value {
    let output = Command::new(env!("CARGO_BIN_EXE_deburr"))
        .args(args)
        .output()
        .expect("run deburr");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    serde_json::from_slice(&output.stdout).expect("decode report")
}

#[test]
fn the_binary_measures_by_default_and_honors_the_duplication_flags() {
    let dir = tempfile::tempdir().expect("temp dir");
    let function = go_function("Alpha", 20);
    write(dir.path(), "a.go", &format!("package a\n\n{function}"));
    write(dir.path(), "b.go", &format!("package b\n\n{function}"));

    let path = dir.path().to_str().expect("utf-8 path");
    let default = deburr(&["audit", path, "--format", "json", "--quiet"]);
    let skipped = deburr(&[
        "audit",
        path,
        "--format",
        "json",
        "--quiet",
        "--no-duplicates",
    ]);
    let strict = deburr(&[
        "audit",
        path,
        "--format",
        "json",
        "--quiet",
        "--min-clone-lines",
        "100",
    ]);
    let tokens = deburr(&[
        "audit",
        path,
        "--format",
        "json",
        "--quiet",
        "--min-clone-tokens",
        "1000",
    ]);

    assert_eq!(default["duplication"]["status"], "measured");
    assert_eq!(
        default["duplication"]["clones"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(skipped["duplication"]["status"], "not_requested");
    assert_eq!(strict["duplication"]["status"], "measured");
    assert_eq!(strict["duplication"]["min_lines"], 100);
    assert_eq!(
        strict["duplication"]["clones"].as_array().map(Vec::len),
        Some(0)
    );
    assert_eq!(tokens["duplication"]["min_tokens"], 1000);
    assert_eq!(
        tokens["duplication"]["clones"].as_array().map(Vec::len),
        Some(0)
    );
}
