//! HTML renderer tests over constructed reports and comparisons.

mod render_fixture;

use deburr::render::{self, Format, Source, Style};
use render_fixture::{
    FailingWriter, assert_shows, comparison_in, report_in, sample_comparison, sample_report,
    sample_report_with_functions,
};

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
fn html_shows_every_report_fact_and_escapes_values() {
    let output = report_in(&sample_report(), Format::Html);

    assert!(
        !output.contains("<script>alert(1)"),
        "function name must be escaped:\n{output}"
    );
    assert_shows(
        Format::Html,
        &output,
        &[
            "Schema <code>3</code>",
            "deburr 1.0.0",
            "source_commit=abc123",
            "source_manifest_digest=sha256:manifest",
            "build_version=1.0.0-test",
            "include_tests=true",
            "<td>7</td><td>5</td><td>1</td><td>2</td><td>3</td><td>1</td><td>4</td><td>6</td>",
            "28.500000",
            "0.250000",
            "null",
            "Function hotspots (1 high-complexity of 2 functions)",
            "Function inventory (2)",
            "native findings (1)",
            "Duplication clones (1)",
            "Primary review items: 3",
            "unreviewed=2, refactored=0, retained=1, deferred=0, stale=1",
            "main.go:1:1-5:2",
            "sloc=4 cyclomatic=12 max_nesting=2 mass=24.000000 flat_guards=1 field_mappings=2 nested_branches=3 assertion_like_calls=4",
            "duplicate-branches",
            "if branches have identical syntax",
            "main.go:4:2-9:3",
            "retained",
            "<code>cohesive-validation</code>: branches are intentionally parallel",
            "read tests",
            "owner-team",
            "fp: finding-fp",
            "(stale)",
            "deferred; reason_category=upstream; reason=generated contract; evidence=schema owner; upstream=schema-team; fingerprint=sha256:old",
            "clone-1",
            "family-1",
            "a/one.go:3:1-14:2",
            "b/two.go:20:1-31:2",
            "pkg/b/two.go::Two, pkg/b/two.go::Three",
            "production: 25.00% duplication (10 of 40 code lines across 2 files)",
            "Status <code>measured</code>",
            "broken,file.go",
            "syntax error at 3:5",
            "parse_error",
        ],
    );
}

#[test]
fn html_has_search_and_category_status_filters() {
    let output = report_in(&sample_report(), Format::Html);

    assert_shows(
        Format::Html,
        &output,
        &[
            "id=\"inventory-search\"",
            "id=\"inventory-view\"",
            "id=\"inventory-kind\"",
            "id=\"inventory-category\"",
            "id=\"inventory-status\"",
            "<option value=\"production\">production</option>",
            "<option value=\"observed\">observed</option>",
            "<option value=\"retained\">retained</option>",
            "data-review-row",
            "data-review=\"false\"",
            "row.dataset.search.toLowerCase()",
            "row.hidden = !matches",
            "Showing <strong id=\"inventory-visible\">3</strong> of 4 inventory rows",
        ],
    );
    assert!(
        !output.contains("src="),
        "HTML must not load external assets"
    );
}

#[test]
fn html_lists_every_function_ranked_by_mass() {
    let report = sample_report_with_functions(25);
    let output = report_in(&report, Format::Html);

    let heaviest = output.find("pkg/main.go::F24").expect("F24 present");
    let lightest = output.find("pkg/main.go::F00").expect("F00 present");
    assert!(heaviest < lightest, "inventory is ranked by mass");
    assert_shows(Format::Html, &output, &["Function inventory (25)"]);
}

#[test]
fn html_comparison_shows_every_fact_and_escapes_values() {
    let output = comparison_in(&sample_comparison(), Format::Html);

    assert!(
        !output.contains("Resolved<b>"),
        "function names must be escaped:\n{output}"
    );
    assert_shows(
        Format::Html,
        &output,
        &[
            "Schema <code>3</code>",
            "compatible <code>true</code>",
            "complete <code>true</code>",
            "source_commit=abc123",
            "source_commit=def456",
            "build_version=1.0.1-test",
            "<th>Before</th><td>5</td><td>3</td><td>1</td><td>2</td><td>1</td>",
            "Delta all",
            "-21.500000",
            "-0.250000",
            "New findings (0)",
            "No longer reported findings (1)",
            "No new findings.",
            "main.go:4:2-9:3",
            "total=3, unreviewed=2, refactored=0, retained=1, deferred=0, stale=1",
            "Finding changes (1)",
            "no_longer_reported",
            "Function changes (1)",
            "resolved_threshold",
            "no behavior conclusion",
            "high_complexity=true",
            "before status=retained reason_category=lifecycle-ordering reason=ordering matters evidence=ran tests",
            "Clone changes (1)",
            "family=family-1",
            "functions=pkg/b/two.go::Two,pkg/b/two.go::Three",
            "Ambiguous matches (1)",
            "two candidates",
            "b1, b2",
            "Before <code>measured</code>",
            "Before clones",
            "min tokens <code>80</code>",
        ],
    );
}
