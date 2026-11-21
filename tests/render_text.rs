//! Text renderer tests over constructed reports and comparisons.

mod render_fixture;

use deburr::render::{self, Format, Source, Style};
use deburr::report::{
    Category, Change, ChangeState, CodeClone, DuplicationStatus, Function, Report,
};
use render_fixture::{
    FailingWriter, comparison_in, report_in, sample_comparison, sample_report,
    sample_report_with_functions,
};
use serde_json::json;

fn assert_shows(format: Format, output: &str, facts: &[&str]) {
    for fact in facts {
        assert!(
            output.contains(fact),
            "{} output is missing {fact:?}:\n{output}",
            format.name()
        );
    }
}

fn assert_hides(output: &str, absent: &[&str]) {
    for text in absent {
        assert!(!output.contains(text), "output shows {text:?}:\n{output}");
    }
}

/// `count` over-limit production functions in distinct files, heaviest last.
fn over_limit_report(count: u32) -> Report {
    let mut report = sample_report();
    report.findings.clear();
    report.review = None;
    report.duplication.clones.clear();
    report.functions = (0..count)
        .map(|index| {
            serde_json::from_value::<Function>(json!({
                "id": format!("pkg/f{index:02}.go::Big{index:02}"),
                "fingerprint": format!("sha256:big{index:02}"),
                "path": format!("pkg/f{index:02}.go"),
                "category": "production",
                "name": format!("Big{index:02}"),
                "structural_signals": {
                    "flat_guards": 0, "field_mappings": 0,
                    "nested_branches": 0, "assertion_like_calls": 0,
            "clone_like_calls": 0, "unwrap_like_calls": 0, "panic_macros": 0, "unsafe_blocks": 0
                },
                "start": { "line": 3, "column": 1 },
                "end": { "line": 40 + index, "column": 2 },
                "sloc": 30 + index,
                "cyclomatic": 14,
                "max_nesting": 2,
                "mass": 90.0,
                "high_complexity": true,
            }))
            .expect("function")
        })
        .collect();

    report
}

#[test]
fn text_reports_writer_errors() {
    let source = Source::default();

    assert!(
        render::render_report(
            &sample_report(),
            &source,
            Style::default(),
            Format::Text,
            &mut FailingWriter
        )
        .is_err(),
        "text swallowed a report write error"
    );
    assert!(
        render::render_comparison(
            &sample_comparison(),
            &source,
            Style::default(),
            Format::Text,
            &mut FailingWriter
        )
        .is_err(),
        "text swallowed a comparison write error"
    );
}

#[test]
fn text_report_leads_with_the_verdict_then_the_worklist() {
    let output = report_in(&over_limit_report(2), Format::Text);

    assert_shows(
        Format::Text,
        &output,
        &[
            "deburr · report @ abc123 · 5 files analyzed (1 vendor/generated skipped)",
            "Complexity",
            "2 functions are over the complexity limit",
            "Duplication",
            "25.0% of production code copied (10 of 40 lines)",
            "Findings",
            "No findings",
            "Start here",
            "Big01 has 14 paths and nests 2 deep",
            "Cyclomatic complexity 14 (limit 10) over 31 lines",
            "pkg/f01.go:3",
            "Full report:  deburr audit . --format html --output deburr.html",
            "For tools:    deburr audit . --format json",
        ],
    );

    let verdict = output.find("Complexity").expect("verdict");
    let worklist = output.find("Start here").expect("worklist");
    let pointers = output.find("Full report:").expect("pointers");
    assert!(
        verdict < worklist && worklist < pointers,
        "order:\n{output}"
    );

    let bigger = output.find("Big01").expect("larger function listed");
    let smaller = output.find("Big00").expect("smaller function listed");
    assert!(bigger < smaller, "items rank by size:\n{output}");
}

#[test]
fn text_report_is_summary_first_not_every_fact() {
    let output = report_in(&sample_report(), Format::Text);

    assert_hides(
        &output,
        &[
            "sha256:",
            "clone-1",
            "family-1",
            "mass=",
            "source_manifest_digest",
            "Provenance:",
            "schema 3",
        ],
    );
    assert!(
        !output.lines().any(|line| {
            line.split(|character: char| !(character.is_ascii_digit() || character == '.'))
                .any(|word| {
                    word.split_once('.')
                        .is_some_and(|(_, decimals)| decimals.len() > 1)
                })
        }),
        "no number prints more than one decimal:\n{output}"
    );
}

#[test]
fn text_report_says_what_the_sample_needs() {
    let output = report_in(&sample_report(), Format::Text);

    assert_shows(
        Format::Text,
        &output,
        &[
            "1 file could not be parsed: broken,file.go",
            "<script>alert(1)</script> has 12 paths and nests 2 deep",
            "main.go:1",
            "a/one.go:3",
            "b/two.go:20",
            "1 item settled in review",
        ],
    );
}

#[test]
fn text_has_no_escape_bytes_without_color() {
    let report = over_limit_report(3);
    let plain = report_in(&report, Format::Text);
    let comparison = comparison_in(&sample_comparison(), Format::Text);

    assert!(
        !plain.contains('\x1b'),
        "plain report has escapes:\n{plain}"
    );
    assert!(!comparison.contains('\x1b'), "plain comparison has escapes");
}

#[test]
fn text_colors_only_when_the_style_asks() {
    let report = over_limit_report(1);
    let mut out = Vec::new();

    render::render_report(
        &report,
        &Source::default(),
        Style { color: true },
        Format::Text,
        &mut out,
    )
    .expect("render report");
    let colored = String::from_utf8(out).expect("utf-8");

    assert!(
        colored.contains("\x1b[33mcomplex\x1b[0m"),
        "complexity is yellow:\n{colored}"
    );
    assert!(
        colored.contains("\x1b[32m●\x1b[0m"),
        "a clear verdict is green:\n{colored}"
    );

    let stripped: String = colored
        .replace("\x1b[33m", "")
        .replace("\x1b[32m", "")
        .replace("\x1b[35m", "")
        .replace("\x1b[31m", "")
        .replace("\x1b[0m", "");
    assert!(!stripped.contains('\x1b'), "only known codes are used");
}

#[test]
fn text_shows_ten_items_then_counts_the_rest() {
    let output = report_in(&over_limit_report(14), Format::Text);

    assert_shows(Format::Text, &output, &["… 4 more", " 10  complex"]);
    assert_hides(&output, &[" 11  complex"]);
    // The four smallest are cut: Big00..Big03.
    assert_hides(&output, &["Big00 ", "Big03 "]);
    assert_shows(Format::Text, &output, &["Big13 ", "Big04 "]);
}

#[test]
fn text_limits_the_worklist_of_many_functions() {
    let mut report = sample_report_with_functions(25);
    report.duplication.clones.clear();
    let output = report_in(&report, Format::Text);

    assert_shows(Format::Text, &output, &["No open items."]);
    assert_hides(&output, &["F24", "hotspots"]);
}

#[test]
fn text_reports_not_requested_duplication() {
    let mut report = sample_report();
    report.duplication.status = DuplicationStatus::NotRequested;
    report.duplication.categories.clear();
    report.duplication.clones.clear();
    let output = report_in(&report, Format::Text);

    assert_shows(
        Format::Text,
        &output,
        &[
            "Duplication was not measured",
            "Clone detection was skipped (--no-duplicates).",
        ],
    );
}

#[test]
fn text_reports_duplication_errors_with_their_reason() {
    let mut report = sample_report();
    report.duplication.status = DuplicationStatus::Error;
    report.duplication.error = "tokenizer exploded".to_owned();
    let output = report_in(&report, Format::Text);

    assert_shows(
        Format::Text,
        &output,
        &["Duplication was not measured", "tokenizer exploded"],
    );
}

#[test]
fn text_hints_when_tooling_directories_dominate() {
    let mut report = over_limit_report(3);
    for (function, directory) in report.functions.iter_mut().zip(["hack", "hack", "src"]) {
        function.path = format!("{directory}/x/{}.go", function.name);
    }

    let output = report_in(&report, Format::Text);

    assert_shows(
        Format::Text,
        &output,
        &[
            "2 of the 3 over-limit functions",
            "hack/x/",
            "marking them as tests would focus this report on shipped code",
        ],
    );
}

#[test]
fn text_ranks_production_before_tests() {
    let mut report = over_limit_report(2);
    report.functions[1].category = Category::Test;
    report.functions[1].sloc = 500;

    let output = report_in(&report, Format::Text);
    let production = output.find("Big00").expect("production item");
    let test = output.find("Big01").expect("test item");

    assert!(production < test, "shipped code ranks first:\n{output}");
    assert_shows(Format::Text, &output, &["[test]"]);
}

#[test]
fn text_comparison_leads_with_the_verdict() {
    let output = comparison_in(&sample_comparison(), Format::Text);

    assert_shows(
        Format::Text,
        &output,
        &[
            "deburr · comparison · 0 got worse, 2 improved",
            "Improved",
            "complex",
            "cyclomatic 12 → cyclomatic 2",
            "main.go:1",
            "duplicate-branches → gone",
        ],
    );
    assert_hides(
        &output,
        &["Got worse", "sha256:", "schema 3", "Compatible:"],
    );
}

#[test]
fn text_comparison_lists_what_got_worse() {
    let mut comparison = sample_comparison();
    let mut crossed = comparison.function_changes[0].clone();
    crossed.state = ChangeState::CrossedThreshold;
    std::mem::swap(&mut crossed.before, &mut crossed.after);
    comparison.function_changes = vec![crossed];

    let output = comparison_in(&comparison, Format::Text);

    assert_shows(
        Format::Text,
        &output,
        &[
            "1 got worse, 1 improved",
            "Got worse",
            "F00  cyclomatic 2 → cyclomatic 12",
        ],
    );
}

#[test]
fn text_comparison_without_a_conclusion_says_why() {
    let mut comparison = sample_comparison();
    comparison.compatible = false;
    comparison.reason = "duplication settings differ".to_owned();
    let output = comparison_in(&comparison, Format::Text);

    assert_shows(
        Format::Text,
        &output,
        &["No conclusion: duplication settings differ"],
    );
    assert_hides(&output, &["Improved", "Got worse"]);
}

#[test]
fn text_comparison_counts_what_could_not_be_matched() {
    let mut comparison = sample_comparison();
    let ambiguous: Change<CodeClone> = Change {
        state: ChangeState::Ambiguous,
        match_basis: None,
        before: None,
        after: None,
        before_review: None,
        after_review: None,
        before_ids: Vec::new(),
        after_ids: Vec::new(),
    };
    comparison.clone_changes = vec![ambiguous.clone(), ambiguous];

    let output = comparison_in(&comparison, Format::Text);

    assert_shows(
        Format::Text,
        &output,
        &["2 items could not be matched between the reports."],
    );
}

#[test]
fn cleanup_guide_describes_built_in_duplication() {
    let mut out = Vec::new();
    render::text::cleanup_guide(&mut out).expect("write guide");
    let guide = String::from_utf8(out).expect("utf-8 guide");

    assert_shows(
        Format::Text,
        &guide,
        &[
            "# Deburr cleanup guide",
            "deburr review init",
            "deburr compare",
            "--no-duplicates",
            "--min-clone-tokens",
            "--min-clone-lines",
            "on by default",
        ],
    );
    for removed in [
        "jscpd",
        "--duplicates-config",
        "--enforce-duplicates-threshold",
        "--cpd",
        ".jscpd.json",
    ] {
        assert!(!guide.contains(removed), "guide still mentions {removed}");
    }
}
