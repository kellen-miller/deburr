//! Rust duplication never joins code across a removed region: production code
//! on either side of an inline test, and inline tests on either side of
//! production code, are separate detector inputs.
//!
//! Every scenario pairs a file whose two halves are split by the other
//! category with a file that holds the same two halves contiguously, so a
//! match across the split would be the only way to see them as one clone.

use std::fs;
use std::io;
use std::path::Path;

use deburr::audit::{AuditOptions, audit};
use deburr::duplication::DuplicationSettings;
use deburr::report::{Category, CodeClone, DuplicationStatus, Report};

/// Tokens in one six-statement half stay above this; a half alone is a clone.
const MIN_TOKENS: usize = 40;

/// Code lines of one production half: signature, six statements, closing brace.
const HALF_CODE_LINES: u32 = 8;

fn options(duplication: DuplicationSettings, include_tests: bool) -> AuditOptions {
    AuditOptions {
        include_tests,
        include_testdata: false,
        include_vendor: false,
        include_generated: false,
        max_file_bytes: None,
        duplication: Some(duplication),
    }
}

fn settings(min_lines: usize) -> DuplicationSettings {
    DuplicationSettings {
        min_tokens: MIN_TOKENS,
        min_lines,
    }
}

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent directories");
    }

    fs::write(path, contents).expect("write fixture");
}

/// A function of six distinct statements; `prefix` keeps halves apart so
/// only the same half matches itself.
fn half(name: &str, prefix: &str) -> String {
    let mut text = format!("fn {name}(input: u32) {{\n");

    for index in 0..6 {
        text.push_str(&format!(
            "    let {prefix}{index} = step_{prefix}{index}(input, {index});\n"
        ));
    }

    text.push_str("}\n");

    text
}

fn inline_test_module(name: &str, body: &str) -> String {
    format!("#[cfg(test)]\nmod {name} {{\n    #[test]\n    fn check() {{\n{body}    }}\n}}\n")
}

fn test_body(prefix: &str) -> String {
    let mut text = String::new();

    for index in 0..6 {
        text.push_str(&format!(
            "        let {prefix}{index} = probe_{prefix}{index}({index});\n"
        ));
    }

    text
}

fn line_of(source: &str, needle: &str) -> u32 {
    let index = source
        .lines()
        .position(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("{needle:?} not in fixture"));

    u32::try_from(index + 1).expect("line fits u32")
}

fn audit_duplication(root: &Path, duplication: DuplicationSettings) -> Report {
    let outcome = audit(root, &options(duplication, true), &mut io::sink());

    assert!(outcome.errors.is_empty(), "errors: {:?}", outcome.errors);
    assert_eq!(
        outcome.report.duplication.status,
        DuplicationStatus::Measured
    );

    outcome.report
}

fn clones_in(report: &Report, category: Category) -> Vec<&CodeClone> {
    report
        .duplication
        .clones
        .iter()
        .filter(|clone| clone.category == category)
        .collect()
}

/// Production: `tail`, an inline test module, `head`; `other.rs` holds `tail`
/// then `head` contiguously.
fn production_fixture(root: &Path) -> String {
    let split = format!(
        "{}\n{}\n{}",
        half("tail", "t"),
        inline_test_module("tests", &test_body("p")),
        half("head", "h"),
    );

    write(root, "src/split.rs", &split);
    write(
        root,
        "src/other.rs",
        &format!("{}\n{}", half("tail", "t"), half("head", "h")),
    );

    split
}

#[test]
fn production_clone_does_not_bridge_a_removed_inline_test() {
    let root = tempfile::tempdir().expect("temp dir");
    production_fixture(root.path());

    // Both halves together are 16 code lines in `other.rs` and, with the
    // inline test removed, in `split.rs`; each half alone is only 8.
    let report = audit_duplication(root.path(), settings(10));

    assert!(
        clones_in(&report, Category::Production).is_empty(),
        "unexpected clones: {:#?}",
        report.duplication.clones
    );
}

#[test]
fn production_clones_stop_at_the_removed_inline_test() {
    let root = tempfile::tempdir().expect("temp dir");
    let split = production_fixture(root.path());
    let tests_start = line_of(&split, "#[cfg(test)]");
    let tests_end = line_of(&split, "probe_p5");
    let report = audit_duplication(root.path(), settings(5));
    let clones = clones_in(&report, Category::Production);

    assert_eq!(clones.len(), 2, "clones: {clones:#?}");

    for clone in clones {
        let [first, second] = &clone.endpoints;

        for endpoint in [first, second] {
            assert_eq!(endpoint.code_lines, HALF_CODE_LINES, "{endpoint:?}");

            if endpoint.path == "src/split.rs" {
                assert!(
                    endpoint.end.line < tests_start || endpoint.start.line > tests_end,
                    "endpoint spans the inline test: {endpoint:?}"
                );
            }
        }
    }
}

#[test]
fn exclude_tests_keeps_the_split_production_halves_apart() {
    let root = tempfile::tempdir().expect("temp dir");
    production_fixture(root.path());

    let outcome = audit(root.path(), &options(settings(10), false), &mut io::sink());

    assert!(outcome.errors.is_empty(), "errors: {:?}", outcome.errors);
    assert_eq!(
        outcome.report.duplication.status,
        DuplicationStatus::Measured
    );
    assert!(
        outcome.report.duplication.clones.is_empty(),
        "unexpected clones: {:#?}",
        outcome.report.duplication.clones
    );
}

/// Inline tests: `first`, a production function, `second`; `tests/other.rs`
/// holds `first` then `second` contiguously.
fn inline_test_fixture(root: &Path) -> String {
    let split = format!(
        "{}\n{}\n{}",
        inline_test_module("first", &test_body("a")),
        half("between", "b"),
        inline_test_module("second", &test_body("c")),
    );

    write(root, "src/split.rs", &split);
    write(
        root,
        "tests/other.rs",
        &format!(
            "{}\n{}",
            inline_test_module("first", &test_body("a")),
            inline_test_module("second", &test_body("c")),
        ),
    );

    split
}

#[test]
fn inline_test_clone_does_not_bridge_production_code() {
    let root = tempfile::tempdir().expect("temp dir");
    inline_test_fixture(root.path());

    // Each module is 12 code lines; the two together are 24.
    let report = audit_duplication(root.path(), settings(13));

    assert!(
        clones_in(&report, Category::Test).is_empty(),
        "unexpected clones: {:#?}",
        report.duplication.clones
    );
}

#[test]
fn inline_test_clones_stop_at_the_production_code() {
    let root = tempfile::tempdir().expect("temp dir");
    let split = inline_test_fixture(root.path());
    let between_start = line_of(&split, "fn between");
    let between_end = line_of(&split, "step_b5");
    let report = audit_duplication(root.path(), settings(5));
    let clones = clones_in(&report, Category::Test);

    assert_eq!(clones.len(), 2, "clones: {clones:#?}");

    for clone in clones {
        for endpoint in &clone.endpoints {
            if endpoint.path == "src/split.rs" {
                assert!(
                    endpoint.end.line < between_start || endpoint.start.line > between_end + 1,
                    "endpoint spans production code: {endpoint:?}"
                );
            }
        }
    }
}

#[test]
fn a_clone_inside_one_contiguous_run_is_still_found() {
    let root = tempfile::tempdir().expect("temp dir");

    write(
        root.path(),
        "src/one.rs",
        &format!("{}\n{}", half("tail", "t"), half("head", "h")),
    );
    write(
        root.path(),
        "src/two.rs",
        &format!("{}\n{}", half("tail", "t"), half("head", "h")),
    );

    let report = audit_duplication(root.path(), settings(10));
    let clones = clones_in(&report, Category::Production);

    assert_eq!(clones.len(), 1, "clones: {clones:#?}");
    assert_eq!(clones[0].endpoints[0].code_lines, 2 * HALF_CODE_LINES);
}
