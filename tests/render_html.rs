//! HTML renderer tests over constructed reports and comparisons.

mod render_fixture;

use std::fs;
use std::path::Path;

use deburr::render::{self, Format, Source, Style};
use deburr::report::{Change, ChangeState, Comparison, Finding, Report};
use render_fixture::{
    FailingWriter, comparison_in, report_in, sample_comparison, sample_report,
    sample_report_with_functions,
};
use serde_json::Value;

const LINK_BASE: &str = "https://github.com/owner/repo/blob/abc123";

fn assert_shows(output: &str, facts: &[&str]) {
    for fact in facts {
        assert!(
            output.contains(fact),
            "html output is missing {fact:?}:\n{output}"
        );
    }
}

fn render_with(report: &Report, source: &Source) -> String {
    let mut out = Vec::new();
    render::render_report(report, source, Style::default(), Format::Html, &mut out)
        .expect("render report");

    String::from_utf8(out).expect("utf-8 output")
}

fn render_comparison_with(comparison: &Comparison, source: &Source) -> String {
    let mut out = Vec::new();
    render::render_comparison(comparison, source, Style::default(), Format::Html, &mut out)
        .expect("render comparison");

    String::from_utf8(out).expect("utf-8 output")
}

/// The JSON inside `<script type="application/json" id="{id}">`, taken up to
/// the first `</script>` the way a browser would.
fn embedded(output: &str, id: &str) -> Value {
    let open = format!("<script type=\"application/json\" id=\"{id}\">");
    let start = output.find(&open).expect("embedded script") + open.len();
    let length = output[start..].find("</script>").expect("script end");

    serde_json::from_str(&output[start..start + length]).expect("embedded JSON parses")
}

/// `filler` lines, then the 12-line body a clone region covers. The body is
/// the same text wherever it sits; `odd` replaces one of its lines.
fn file_with_body(filler: u32, odd: Option<u32>) -> String {
    let filler = (1..=filler).map(|number| format!("filler {number}\n"));
    let body = (1..=12).map(|number| {
        if odd == Some(number) {
            "different body line\n".to_owned()
        } else {
            format!("body {number}\n")
        }
    });

    filler.chain(body).collect()
}

/// A source root holding every file the sample report refers to. The two
/// clone regions (a/one.go:3-14 and b/two.go:20-31) differ on one line.
fn source_root(dir: &Path) {
    fs::create_dir_all(dir.join("a")).expect("mkdir a");
    fs::create_dir_all(dir.join("b")).expect("mkdir b");

    let main: String = (1..=12)
        .map(|number| format!("main line {number}\n"))
        .collect();

    fs::write(dir.join("main.go"), main).expect("main.go");
    fs::write(dir.join("a/one.go"), file_with_body(2, None)).expect("one.go");
    fs::write(dir.join("b/two.go"), file_with_body(19, Some(7))).expect("two.go");
}

#[test]
fn html_reports_writer_errors() {
    let source = Source::default();

    assert!(
        render::render_report(
            &sample_report(),
            &source,
            Style::default(),
            Format::Html,
            &mut FailingWriter
        )
        .is_err(),
        "html swallowed a report write error"
    );
    assert!(
        render::render_comparison(
            &sample_comparison(),
            &source,
            Style::default(),
            Format::Html,
            &mut FailingWriter
        )
        .is_err(),
        "html swallowed a comparison write error"
    );
}

#[test]
fn html_report_leads_with_a_verdict_and_a_worklist() {
    let output = report_in(&sample_report(), Format::Html);

    assert_shows(
        &output,
        &[
            "<h1>report</h1>",
            "1 function is over the complexity limit",
            "Start here",
            "Copied code",
            "Where it concentrates",
            "Complex functions",
            "All functions",
            "Audit details",
            "<button class=\"btn\" id=\"copy-selector\"",
        ],
    );
    let page = &output[..output
        .find("<script type=\"application/json\"")
        .expect("json")];

    assert!(
        !page.contains("sha256:clone") && !page.contains("clone-1"),
        "clone IDs and fingerprints belong in the JSON, not the page:\n{page}"
    );
    assert!(
        output.find("Start here").expect("worklist") < output.find("Audit details").expect("audit"),
        "the worklist comes before the audit details"
    );
}

#[test]
fn html_escapes_report_text_in_the_page_and_the_embedded_json() {
    let mut report = sample_report();
    report.functions[0].name = "</script><b>x".to_owned();
    report.findings[0].message = "</script><i>y".to_owned();

    let output = report_in(&report, Format::Html);

    assert!(
        !output.contains("</script><b>") && !output.contains("</script><i>"),
        "report text must not reach the page unescaped:\n{output}"
    );
    assert_shows(&output, &["&#60;/script&#62;&#60;b&#62;x"]);
    assert_shows(&output, &["\\u003c/script>\\u003cb>x"]);

    let json = embedded(&output, "deburr-report");

    assert_eq!(json["schema_version"], "4");
    assert_eq!(json["functions"][0]["name"], "</script><b>x");
    assert_eq!(json["findings"][0]["message"], "</script><i>y");
}

#[test]
fn html_loads_nothing_from_the_network() {
    let output = report_in(&sample_report(), Format::Html);

    for needle in ["src=", "href=\"http", "@import", "url(", "<link"] {
        assert!(
            !output.contains(needle),
            "report page contains {needle:?}:\n{output}"
        );
    }

    assert_shows(
        &output,
        &[
            "prefers-color-scheme: dark",
            ":root[data-theme=\"dark\"]",
            "name=\"viewport\"",
        ],
    );
}

#[test]
fn html_shows_source_when_the_root_has_the_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    source_root(dir.path());

    let source = Source::new(Some(dir.path().to_path_buf()), None, String::new());
    let output = render_with(&sample_report(), &source);

    assert_shows(
        &output,
        &[
            "<span class=\"l\" data-n=\"1\">main line 1</span>",
            "<span class=\"l\" data-n=\"5\">main line 5</span>",
            "Show the function",
        ],
    );
    assert!(
        !output.contains("Source not available"),
        "every file exists:\n{output}"
    );
}

#[test]
fn html_says_when_source_is_not_available() {
    let report = sample_report();
    let without_root = render_with(&report, &Source::default());
    let dir = tempfile::tempdir().expect("tempdir");
    let missing_file = render_with(
        &report,
        &Source::new(Some(dir.path().to_path_buf()), None, String::new()),
    );

    for output in [without_root, missing_file] {
        assert_shows(
            &output,
            &["Source not available when this report was rendered."],
        );
        assert!(
            !output.contains("<pre class=\"src\">"),
            "no source block without source:\n{output}"
        );
    }
}

#[test]
fn html_links_locations_only_with_a_link_base() {
    let report = sample_report();
    let plain = render_with(&report, &Source::default());
    let linked = render_with(
        &report,
        &Source::new(None, Some(LINK_BASE.to_owned()), "svc/".to_owned()),
    );

    assert!(
        !plain.contains("href=\"https://"),
        "no link base, no links:\n{plain}"
    );
    assert_shows(
        &linked,
        &[
            &format!("href=\"{LINK_BASE}/svc/main.go#L1-L5\""),
            &format!("href=\"{LINK_BASE}/svc/a/one.go#L3-L14\""),
            &format!("href=\"{LINK_BASE}/svc/b/two.go#L20-L31\""),
        ],
    );
    assert_shows(&linked, &[&format!("data-link=\"{LINK_BASE}/svc/")]);
    assert_shows(&plain, &["data-link=\"\""]);
}

#[test]
fn html_highlights_the_lines_that_differ_between_two_copies() {
    let dir = tempfile::tempdir().expect("tempdir");
    source_root(dir.path());

    let source = Source::new(Some(dir.path().to_path_buf()), None, String::new());
    let output = render_with(&sample_report(), &source);

    assert_shows(&output, &["Compare the copies", "different body line"]);
    // The regions sit on different line numbers but hold the same text,
    // except for one line, which is highlighted on both sides.
    assert_eq!(
        output.matches("class=\"l d\"").count(),
        2,
        "one differing line, highlighted on both sides:\n{output}"
    );
}

#[test]
fn html_hides_settled_items_and_offers_to_show_them() {
    let output = report_in(&sample_report(), Format::Html);

    // The retained finding is out of the worklist and behind a toggle.
    assert_shows(
        &output,
        &[
            "1 settled in review is hidden",
            "<summary>Show 1 settled item</summary>",
            "<span class=\"chip ok\">retained</span>",
        ],
    );

    let worklist = &output[output.find("<ol class=\"work\">").expect("worklist")
        ..output.find("</ol>").expect("worklist end")];

    assert!(
        !worklist.contains("duplicate-branches"),
        "settled finding stays out of the worklist:\n{worklist}"
    );
}

#[test]
fn html_writes_fifty_function_rows_and_leaves_the_rest_to_the_script() {
    let report = sample_report_with_functions(120);
    let output = report_in(&report, Format::Html);

    assert_shows(
        &output,
        &[
            "120 functions, heaviest first",
            "<code>F119</code>",
            "<code>F70</code>",
        ],
    );
    assert!(
        !output.contains("<code>F69</code>"),
        "row 51 is built by the script:\n{output}"
    );
    assert_eq!(
        embedded(&output, "deburr-report")["functions"][5]["name"],
        "F05"
    );
}

#[test]
fn html_shows_source_for_at_most_three_hundred_items() {
    let dir = tempfile::tempdir().expect("tempdir");
    source_root(dir.path());

    let mut report = sample_report();
    let hotspot = report.functions[0].clone();

    report.duplication.clones.clear();
    report.functions = (0..305)
        .map(|index| {
            let mut function = hotspot.clone();
            function.id = format!("pkg/main.go::H{index}");
            function.name = format!("H{index}");

            function
        })
        .collect();

    let source = Source::new(Some(dir.path().to_path_buf()), None, String::new());
    let output = render_with(&report, &source);

    assert_shows(&output, &["Source shown for the first 300 items."]);
    assert_eq!(
        output.matches("<pre class=\"src\">").count(),
        300,
        "source stops at the cap"
    );
    assert_shows(&output, &["305 to look at"]);
}

#[test]
fn html_lists_copied_code_groups() {
    let output = report_in(&sample_report(), Format::Html);

    assert_shows(
        &output,
        &[
            "<details class=\"grp\">",
            "1 production and 0 test groups",
            "a/one.go:3",
            "b/two.go:20",
        ],
    );
}

#[test]
fn html_comparison_leads_with_the_verdict_and_escapes_values() {
    let output = comparison_in(&sample_comparison(), Format::Html);

    assert!(
        !output.contains("Resolved<b>"),
        "function names must be escaped:\n{output}"
    );
    assert_shows(
        &output,
        &[
            "0 got worse, 2 improved",
            "Improved (2)",
            "Nothing got worse.",
            "Resolved&#60;b&#62;",
            "cyclomatic 12",
            "cyclomatic 2",
            "abc123 → def456",
            "Audit details",
            "cyclomatic 10",
            "80 tokens and 8 code lines",
        ],
    );

    let json = embedded(&output, "deburr-comparison");

    assert_eq!(json["schema_version"], "4");
    assert_eq!(json["function_changes"][0]["after"]["name"], "Resolved<b>");
}

#[test]
fn html_comparison_says_when_no_conclusion_can_be_drawn() {
    let mut comparison = sample_comparison();
    comparison.compatible = false;
    comparison.reason = "clone settings differ".to_owned();

    let output = comparison_in(&comparison, Format::Html);

    assert_shows(
        &output,
        &[
            "No conclusion: clone settings differ",
            "cannot support a conclusion",
        ],
    );
    assert!(
        !output.contains("Got worse"),
        "no lists without a conclusion:\n{output}"
    );
}

#[test]
fn html_comparison_counts_items_it_could_not_match() {
    let mut comparison = sample_comparison();
    comparison.finding_changes.push(Change::<Finding> {
        state: ChangeState::Ambiguous,
        match_basis: None,
        before: None,
        after: None,
        before_review: None,
        after_review: None,
        before_ids: Vec::new(),
        after_ids: Vec::new(),
    });

    let output = comparison_in(&comparison, Format::Html);

    assert_shows(
        &output,
        &["1 item could not be matched between the reports."],
    );
}

#[test]
fn html_comparison_shows_code_only_for_what_still_exists() {
    let dir = tempfile::tempdir().expect("tempdir");
    source_root(dir.path());

    let mut comparison = sample_comparison();
    comparison.new_findings = comparison.resolved_findings.clone();

    let source = Source::new(Some(dir.path().to_path_buf()), None, String::new());
    let output = render_comparison_with(&comparison, &source);

    // The new finding and the resolved function are in the newer tree. The
    // resolved finding is gone, so it has no code.
    assert_shows(
        &output,
        &[
            "1 got worse, 2 improved",
            "<span class=\"l\" data-n=\"4\">main line 4</span>",
        ],
    );
    assert_eq!(
        output.matches("<pre class=\"src\">").count(),
        2,
        "code for the new finding and the resolved function only:\n{output}"
    );
}
