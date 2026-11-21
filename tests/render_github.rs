//! GitHub annotation renderer tests over constructed reports and comparisons.

mod render_fixture;

use deburr::render::{self, Format, Source, Style};
use deburr::report::{Change, ChangeState, Comparison, Finding, Function, Report};
use render_fixture::{
    FailingWriter, comparison_in, report_in, sample_comparison, sample_report,
    sample_report_with_functions,
};
use serde_json::json;

/// An over-limit function `Big{index}` in `big.go`, `sloc` lines long.
fn big_function(index: u32, sloc: u32) -> Function {
    serde_json::from_value(json!({
        "id": format!("pkg/big.go::Big{index:02}"),
        "fingerprint": format!("sha256:big{index:02}"),
        "path": "big.go",
        "category": "production",
        "name": format!("Big{index:02}"),
        "structural_signals": {
            "flat_guards": 0, "field_mappings": 0, "nested_branches": 0, "assertion_like_calls": 0,
            "clone_like_calls": 0, "unwrap_like_calls": 0, "panic_macros": 0, "unsafe_blocks": 0
        },
        "start": { "line": index * 100 + 1, "column": 1 },
        "end": { "line": index * 100 + sloc, "column": 2 },
        "sloc": sloc,
        "cyclomatic": 20,
        "max_nesting": 1,
        "mass": 90.0,
        "high_complexity": true,
    }))
    .expect("function")
}

/// A report with only over-limit functions: no findings, no clones, no ledger.
fn report_with_big_functions(count: u32) -> Report {
    let mut report = sample_report();
    report.review = None;
    report.findings = Vec::new();
    report.duplication.clones = Vec::new();
    report.functions = (0..count).map(|index| big_function(index, 30)).collect();

    report
}

fn annotations(output: &str) -> Vec<&str> {
    output
        .lines()
        .filter(|line| line.starts_with("::warning") || line.starts_with("::error"))
        .collect()
}

fn assert_shows(format: Format, output: &str, facts: &[&str]) {
    for fact in facts {
        assert!(
            output.contains(fact),
            "{} output is missing {fact:?}:\n{output}",
            format.name()
        );
    }
}

#[test]
fn github_reports_writer_errors() {
    let source = Source::default();

    assert!(
        render::render_report(
            &sample_report(),
            &source,
            Style::default(),
            Format::Github,
            &mut FailingWriter
        )
        .is_err(),
        "github swallowed a report write error"
    );
    assert!(
        render::render_comparison(
            &sample_comparison(),
            &source,
            Style::default(),
            Format::Github,
            &mut FailingWriter
        )
        .is_err(),
        "github swallowed a comparison write error"
    );
}

#[test]
fn github_never_raises_a_warning_for_functions_under_the_limit() {
    let mut report = sample_report_with_functions(25);
    report.findings = Vec::new();
    report.duplication.clones = Vec::new();
    let output = report_in(&report, Format::Github);

    assert!(
        annotations(&output).is_empty(),
        "ordinary functions must not be annotated:\n{output}"
    );
    assert_eq!(output.lines().count(), 1, "only the verdict:\n{output}");
}

#[test]
fn github_report_opens_with_one_verdict_notice() {
    let output = report_in(&report_with_big_functions(3), Format::Github);
    let first = output.lines().next().expect("a first line");

    assert!(
        first.starts_with("::notice title=deburr::3 functions are over the complexity limit"),
        "{first}"
    );
    assert_eq!(
        output
            .lines()
            .filter(|line| line.starts_with("::notice"))
            .count(),
        1,
        "{output}"
    );
    assert_eq!(annotations(&output).len(), 3, "{output}");
}

#[test]
fn github_report_caps_annotations_and_counts_the_rest() {
    let output = report_in(&report_with_big_functions(25), Format::Github);

    assert_eq!(annotations(&output).len(), 10, "{output}");
    assert_eq!(
        output.lines().last().expect("a last line"),
        "::notice title=deburr::15 more items not annotated. See the job summary or the HTML report for the full list."
    );
    assert!(
        output.contains("::warning file=big.go,line=1,endLine=30,title=deburr%3A Big00::"),
        "{output}"
    );
}

#[test]
fn github_report_without_overflow_has_no_trailing_notice() {
    let output = report_in(&report_with_big_functions(10), Format::Github);

    assert_eq!(annotations(&output).len(), 10);
    assert!(!output.contains("not annotated"), "{output}");
}

#[test]
fn github_annotates_copied_code_on_the_first_endpoint_and_names_the_others() {
    let mut report = sample_report();
    report.review = None;
    report.findings = Vec::new();
    report.functions = Vec::new();
    let output = report_in(&report, Format::Github);

    let line = annotations(&output)
        .into_iter()
        .find(|line| line.contains("file=a/one.go"))
        .unwrap_or_else(|| panic!("no copied-code annotation:\n{output}"));

    assert!(
        line.starts_with("::warning file=a/one.go,line=3,endLine=14,"),
        "{line}"
    );
    assert!(line.contains("Also at b/two.go:20"), "{line}");
}

#[test]
fn github_uses_the_finding_message_and_advice() {
    let mut report = sample_report();
    report.review = None;
    report.functions = Vec::new();
    report.duplication.clones = Vec::new();
    let output = report_in(&report, Format::Github);

    assert_shows(
        Format::Github,
        &output,
        &[
            "::warning file=main.go,line=4,endLine=9,title=deburr%3A duplicate-branches::if branches have identical syntax",
            "Merge the identical branches.",
        ],
    );
}

#[test]
fn github_escapes_properties_and_messages() {
    let mut report = sample_report();
    report.review = None;
    report.functions = Vec::new();
    report.duplication.clones = Vec::new();
    report.findings[0].path = "dir,a:b\nc.go".to_owned();
    report.findings[0].message = "bad:%\nnext".to_owned();
    report.findings[0].rule = "title:with,commas".to_owned();
    let output = report_in(&report, Format::Github);

    assert_shows(
        Format::Github,
        &output,
        &[
            "file=dir%2Ca%3Ab%0Ac.go",
            "title=deburr%3A title%3Awith%2Ccommas",
            "bad:%25%0Anext",
        ],
    );
}

#[test]
fn github_maps_finding_severity_to_annotation_level() {
    let mut report = sample_report();
    report.review = None;
    report.functions = Vec::new();
    report.duplication.clones = Vec::new();
    let template: Finding = report.findings[0].clone();
    report.findings = ["error", "NOTICE", "review", "warning"]
        .iter()
        .enumerate()
        .map(|(index, severity)| Finding {
            id: format!("f{index}"),
            start: deburr::report::Position {
                line: u32::try_from(index).expect("small index") + 1,
                column: 1,
            },
            severity: (*severity).to_owned(),
            ..template.clone()
        })
        .collect();
    let output = report_in(&report, Format::Github);

    assert_shows(
        Format::Github,
        &output,
        &[
            "::error file=main.go,line=1,",
            "::notice file=main.go,line=2,",
            "::warning file=main.go,line=3,",
            "::warning file=main.go,line=4,",
        ],
    );
}

#[test]
fn github_verdict_mentions_findings_settled_in_review() {
    let output = report_in(&sample_report(), Format::Github);

    assert!(output.contains("1 items settled in review."), "{output}");
}

/// The sample comparison with every change replaced by `changes`.
fn comparison_with(changes: Vec<Change<Function>>) -> Comparison {
    let mut comparison = sample_comparison();
    comparison.function_changes = changes;
    comparison.finding_changes = Vec::new();
    comparison.clone_changes = Vec::new();
    comparison.new_findings = Vec::new();
    comparison.resolved_findings = Vec::new();
    comparison.ambiguous_findings = Vec::new();

    comparison
}

fn change(
    state: ChangeState,
    before: Option<Function>,
    after: Option<Function>,
) -> Change<Function> {
    Change {
        state,
        match_basis: None,
        before,
        after,
        before_review: None,
        after_review: None,
        before_ids: Vec::new(),
        after_ids: Vec::new(),
    }
}

#[test]
fn github_comparison_annotates_only_worse_entries() {
    let comparison = comparison_with(vec![
        change(ChangeState::New, None, Some(big_function(1, 30))),
        change(
            ChangeState::ResolvedThreshold,
            Some(big_function(2, 30)),
            Some(Function {
                high_complexity: false,
                cyclomatic: 4,
                ..big_function(2, 30)
            }),
        ),
        change(
            ChangeState::DeletedFunction,
            Some(big_function(3, 30)),
            None,
        ),
    ]);
    let output = comparison_in(&comparison, Format::Github);
    let warnings = annotations(&output);

    assert_eq!(
        output.lines().next().expect("a first line"),
        "::notice title=deburr::1 got worse, 2 improved"
    );
    assert_eq!(warnings.len(), 1, "{output}");
    assert!(
        warnings[0]
            .starts_with("::warning file=big.go,line=101,endLine=130,title=deburr%3A Big01::"),
        "{}",
        warnings[0]
    );
    assert!(warnings[0].contains("cyclomatic 20"), "{}", warnings[0]);
    assert!(
        !output.contains("Big02") && !output.contains("Big03"),
        "{output}"
    );
}

#[test]
fn github_comparison_caps_worse_annotations() {
    let comparison = comparison_with(
        (0..12)
            .map(|index| change(ChangeState::New, None, Some(big_function(index, 30))))
            .collect(),
    );
    let output = comparison_in(&comparison, Format::Github);

    assert_eq!(annotations(&output).len(), 10, "{output}");
    assert_eq!(
        output.lines().last().expect("a last line"),
        "::notice title=deburr::2 more items that got worse not annotated. See the job summary for the full list."
    );
}

#[test]
fn github_comparison_without_a_conclusion_annotates_nothing() {
    let mut comparison = comparison_with(vec![change(
        ChangeState::New,
        None,
        Some(big_function(1, 30)),
    )]);
    comparison.compatible = false;
    comparison.reason = "scopes differ".to_owned();
    let output = comparison_in(&comparison, Format::Github);

    assert_eq!(
        output,
        "::notice title=deburr::No conclusion: scopes differ\n"
    );
}

#[test]
fn github_comparison_counts_unmatched_items_in_one_sentence() {
    let comparison = comparison_with(vec![change(
        ChangeState::Ambiguous,
        Some(big_function(1, 30)),
        Some(big_function(1, 30)),
    )]);
    let output = comparison_in(&comparison, Format::Github);

    assert_eq!(
        output,
        "::notice title=deburr::0 got worse, 0 improved. 1 items could not be matched between the reports\n"
    );
}
