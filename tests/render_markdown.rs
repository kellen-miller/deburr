//! Markdown renderer tests over constructed reports and comparisons.

mod render_fixture;

use std::path::PathBuf;

use deburr::render::{self, Format, Source, Style};
use deburr::report::{ChangeState, Comparison, Function, Report};
use render_fixture::{
    FailingWriter, comparison_in, report_in, sample_comparison, sample_report,
    sample_report_with_functions,
};
use serde_json::json;

const LINK_BASE: &str = "https://github.com/o/r/blob/0123456789abcdef0123456789abcdef01234567";

fn assert_shows(format: Format, output: &str, facts: &[&str]) {
    for fact in facts {
        assert!(
            output.contains(fact),
            "{} output is missing {fact:?}:\n{output}",
            format.name()
        );
    }
}

fn with_source(report: &Report, source: &Source) -> String {
    let mut out = Vec::new();

    render::render_report(report, source, Style::default(), Format::Markdown, &mut out)
        .expect("render report");

    String::from_utf8(out).expect("utf-8 output")
}

/// `count` over-limit production functions in distinct files.
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
                    "nested_branches": 0, "assertion_like_calls": 0
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

fn linked_source() -> Source {
    Source::new(
        Some(PathBuf::from(".")),
        Some(LINK_BASE.to_owned()),
        String::new(),
    )
    .named("repo")
}

#[test]
fn markdown_reports_writer_errors() {
    let source = Source::default();

    assert!(
        render::render_report(
            &sample_report(),
            &source,
            Style::default(),
            Format::Markdown,
            &mut FailingWriter
        )
        .is_err(),
        "markdown swallowed a report write error"
    );
    assert!(
        render::render_comparison(
            &sample_comparison(),
            &source,
            Style::default(),
            Format::Markdown,
            &mut FailingWriter
        )
        .is_err(),
        "markdown swallowed a comparison write error"
    );
}

#[test]
fn markdown_report_has_the_verdict_worklist_and_footer() {
    let output = with_source(&over_limit_report(2), &Source::default().named("repo"));

    assert_shows(
        Format::Markdown,
        &output,
        &[
            "## deburr · repo\n",
            "| **Complexity** | 2 functions are over the complexity limit",
            "| **Duplication** | 25.0% of production code copied (10 of 40 lines)",
            "| **Findings** | No findings",
            "### Start here",
            "| # | Item | Why | Where |",
            "| 1 | **complex** Big01 has 14 paths and nests 2 deep",
            "| 2 | **complex** Big00",
            "`pkg/f01.go:3`",
            "### Where it concentrates",
            "<!-- deburr report schema 3 -->",
        ],
    );
    assert!(
        output
            .trim_end()
            .ends_with("deburr 1.0.0 · abc123 <!-- deburr report schema 3 -->"),
        "footer names version, commit, and schema:\n{output}"
    );
    assert!(!output.contains("<details>"), "two items fit in the table");
}

#[test]
fn markdown_links_locations_when_the_source_has_a_base() {
    let output = with_source(&over_limit_report(1), &linked_source());

    assert_shows(
        Format::Markdown,
        &output,
        &[&format!("[pkg/f00.go:3]({LINK_BASE}/pkg/f00.go#L3-L40)")],
    );
    assert!(
        !output.contains("`pkg/f00.go:3`"),
        "linked cells are not code"
    );
}

#[test]
fn markdown_falls_back_to_code_locations_without_links() {
    let output = with_source(&over_limit_report(1), &Source::default());

    assert_shows(Format::Markdown, &output, &["`pkg/f00.go:3`"]);
    assert!(!output.contains("https://"), "no links without a base");
}

#[test]
fn markdown_folds_items_past_ten_into_details() {
    let output = with_source(&over_limit_report(13), &Source::default());

    assert_shows(
        Format::Markdown,
        &output,
        &[
            "<details><summary>3 more items</summary>",
            "</details>",
            "| 11 | **complex**",
            "| 13 | **complex**",
        ],
    );

    let table = output.find("| 10 | **complex**").expect("tenth row");
    let details = output.find("<details>").expect("details");
    let eleventh = output.find("| 11 | **complex**").expect("eleventh row");
    assert!(table < details && details < eleventh, "order:\n{output}");
}

#[test]
fn markdown_escapes_report_text() {
    let mut report = over_limit_report(1);
    report.functions[0].name = "a|b<script>`c`*d".to_owned();

    let output = with_source(&report, &Source::default());

    assert_shows(
        Format::Markdown,
        &output,
        &["a\\|b\\<script\\>\\`c\\`\\*d has 14 paths"],
    );
    assert!(
        !output.contains("<script>"),
        "report text never forms a tag:\n{output}"
    );
}

#[test]
fn markdown_shows_hints_as_quotes_and_settled_counts() {
    let output = with_source(&sample_report(), &Source::default());

    assert_shows(
        Format::Markdown,
        &output,
        &[
            "> 1 file could not be parsed: broken,file.go.",
            "1 item settled in review.",
        ],
    );
}

#[test]
fn markdown_says_when_duplication_was_not_measured() {
    let mut report = sample_report();
    report.duplication.status = deburr::report::DuplicationStatus::NotRequested;
    report.duplication.categories.clear();
    report.duplication.clones.clear();

    let output = report_in(&report, Format::Markdown);

    assert_shows(
        Format::Markdown,
        &output,
        &["Duplication was not measured", "--no-duplicates"],
    );
}

#[test]
fn markdown_has_no_escape_bytes_or_raw_ids() {
    let output = report_in(&sample_report(), Format::Markdown);

    assert!(!output.contains('\x1b'));
    for hidden in ["sha256:", "clone-1", "family-1"] {
        assert!(!output.contains(hidden), "output shows {hidden:?}");
    }
}

#[test]
fn markdown_comparison_marks_better_and_worse_rows() {
    let better = comparison_in(&sample_comparison(), Format::Markdown);

    assert_shows(
        Format::Markdown,
        &better,
        &[
            "### This pull request",
            "**0 got worse, 2 improved**",
            "| | Item | Before | After |",
            "| ✅ | **complex** Resolved\\<b\\>",
            "| cyclomatic 12 | cyclomatic 2 |",
        ],
    );
    assert!(!better.contains("🔺"), "nothing got worse:\n{better}");

    let mut comparison: Comparison = sample_comparison();
    let mut crossed = comparison.function_changes[0].clone();
    crossed.state = ChangeState::CrossedThreshold;
    std::mem::swap(&mut crossed.before, &mut crossed.after);
    comparison.function_changes = vec![crossed];

    let worse = comparison_in(&comparison, Format::Markdown);

    assert_shows(
        Format::Markdown,
        &worse,
        &[
            "**1 got worse, 1 improved**",
            "| 🔺 | **complex** F00<br>`main.go:1` | cyclomatic 2 | cyclomatic 12 |",
            "| ✅ | **finding**",
        ],
    );
    assert_eq!(
        worse.matches("| | Item | Before | After |").count(),
        1,
        "one table holds both lists:\n{worse}"
    );
}

#[test]
fn markdown_names_an_empty_worklist() {
    let mut report = sample_report_with_functions(25);
    report.duplication.clones.clear();
    let output = report_in(&report, Format::Markdown);

    assert_shows(Format::Markdown, &output, &["No open items."]);
    assert!(
        !output.contains("### Start here"),
        "no empty table:\n{output}"
    );
}

#[test]
fn markdown_comparison_without_a_conclusion_says_why() {
    let mut comparison = sample_comparison();
    comparison.compatible = false;
    comparison.reason = "duplication settings differ".to_owned();

    let output = comparison_in(&comparison, Format::Markdown);

    assert_shows(
        Format::Markdown,
        &output,
        &["**No conclusion: duplication settings differ**"],
    );
    assert!(!output.contains("| |"), "no table without a conclusion");
}

#[test]
fn markdown_comparison_links_the_after_location() {
    let mut out = Vec::new();

    render::render_comparison(
        &sample_comparison(),
        &linked_source(),
        Style::default(),
        Format::Markdown,
        &mut out,
    )
    .expect("render comparison");
    let output = String::from_utf8(out).expect("utf-8 output");

    assert_shows(
        Format::Markdown,
        &output,
        &[&format!("[main.go:1]({LINK_BASE}/main.go#L1-L5)")],
    );
}
