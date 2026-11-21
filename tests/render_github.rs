//! GitHub annotation renderer tests over constructed reports and comparisons.

mod render_fixture;

use deburr::render::{self, Format, Source, Style, github};
use deburr::report::Finding;
use render_fixture::{
    FailingWriter, assert_shows, comparison_in, report_in, sample_comparison, sample_report,
    sample_report_with_functions,
};

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
    let output = report_in(&sample_report_with_functions(25), Format::Github);

    assert!(
        output
            .lines()
            .filter(|line| line.starts_with("::warning") || line.starts_with("::error"))
            .all(|line| !line.contains("title=deburr function")),
        "ordinary functions must not be warnings:\n{output}"
    );
}

#[test]
fn github_shows_every_report_fact() {
    let output = report_in(&sample_report(), Format::Github);

    assert_shows(
        Format::Github,
        &output,
        &[
            "::notice title=deburr provenance::build_version=1.0.0-test source_commit=abc123",
            "::notice title=deburr coverage::discovered=7,analyzed=5,excluded=1,unsupported=2,read_errors=3,parse_errors=1,symlinks=4,excluded_directories=6",
            "::error title=deburr analysis::analysis incomplete: read_errors=3,parse_errors=1",
            "::error file=broken%2Cfile.go,title=deburr analysis::syntax error at 3:5",
            "::notice title=deburr duplication::status=measured,min_tokens=80,min_lines=8",
            "production: 25.00%25 duplication (10 of 40 code lines across 2 files)",
            "::notice file=main.go,line=1,endLine=5,title=deburr function::<script>alert(1)</script> sloc=4 cyclomatic=12 nesting=2 mass=24.000000 flat_guards=1 field_mappings=2 nested_branches=3 assertion_like_calls=4 identity_ambiguous=false",
            "::warning file=main.go,line=4,endLine=9,title=duplicate-branches::if branches have identical syntax",
            "::warning file=a/one.go,line=3,endLine=14,title=deburr duplication::clone clone-1 family=family-1 tokens=95 identity_ambiguous=true code_lines=10 functions=pkg/a/one.go::One",
            "::warning file=b/two.go,line=20,endLine=31,title=deburr duplication::clone clone-1 family=family-1 tokens=95 identity_ambiguous=true code_lines=9 functions=pkg/b/two.go::Two,pkg/b/two.go::Three",
            "::notice title=deburr review::total=3,unreviewed=2,refactored=0,retained=1,deferred=0,stale=1",
            "::notice file=main.go,line=4,endLine=9,title=deburr review::finding finding-1 status=retained reason_category=cohesive-validation reason=branches are intentionally parallel evidence=read tests upstream=owner-team",
            "stale=true",
            "previous=deferred; reason_category=upstream",
        ],
    );
    assert!(
        !output.contains("finding clone-1 status=unreviewed"),
        "untouched review items are covered by the summary"
    );
}

#[test]
fn github_escapes_properties_and_messages() {
    let mut report = sample_report();
    report.findings[0].path = "dir,a:b\nc.go".to_owned();
    report.findings[0].message = "bad:%\nnext".to_owned();
    report.findings[0].rule = "title:with,commas".to_owned();
    let output = report_in(&report, Format::Github);

    assert_shows(
        Format::Github,
        &output,
        &[
            "file=dir%2Ca%3Ab%0Ac.go",
            "title=title%3Awith%2Ccommas",
            "bad:%25%0Anext",
        ],
    );
}

#[test]
fn github_maps_severity_to_annotation_level() {
    let mut report = sample_report();
    let template: Finding = report.findings[0].clone();
    report.findings = ["error", "NOTICE", "review", "warning"]
        .iter()
        .enumerate()
        .map(|(index, severity)| Finding {
            id: format!("f{index}"),
            severity: (*severity).to_owned(),
            ..template.clone()
        })
        .collect();
    let output = report_in(&report, Format::Github);

    assert_shows(
        Format::Github,
        &output,
        &[
            "::error file=main.go",
            "::notice file=main.go,line=4,endLine=9,title=duplicate-branches",
            "::warning file=main.go,line=4,endLine=9,title=duplicate-branches",
        ],
    );
    assert_eq!(output.matches("title=duplicate-branches").count(), 4);
}

#[test]
fn github_paths_can_be_prefixed_for_subdirectory_audits() {
    let prefixed = github::with_path_prefix(&sample_report(), "services/api");
    let output = report_in(&prefixed, Format::Github);

    assert_shows(
        Format::Github,
        &output,
        &[
            "file=services/api/main.go,line=4",
            "file=services/api/a/one.go,line=3",
            "file=services/api/broken%2Cfile.go",
            "file=services/api/main.go,line=1,endLine=5,title=deburr review",
        ],
    );

    assert_eq!(
        github::with_path_prefix(&sample_report(), ""),
        sample_report(),
        "an empty prefix leaves the report unchanged"
    );
}

#[test]
fn github_comparison_shows_every_fact() {
    let output = comparison_in(&sample_comparison(), Format::Github);

    assert_shows(
        Format::Github,
        &output,
        &[
            "title=deburr comparison before provenance::build_version=1.0.0-test source_commit=abc123",
            "title=deburr comparison after provenance::build_version=1.0.1-test source_commit=def456",
            "title=deburr comparison::compatible=true,complete=true,before_analyzed=5,after_analyzed=5",
            "title=deburr comparison review::before_total=3,before_unreviewed=2,before_refactored=0,before_retained=1,before_deferred=0,before_stale=1,after_total=3",
            "::notice file=main.go,line=4,endLine=9,title=duplicate-branches::no_longer_reported: if branches have identical syntax",
            "::notice file=main.go,line=1,endLine=5,title=deburr function comparison::Resolved<b> state=resolved_threshold; no behavior conclusion",
            "::notice file=a/one.go,line=3,endLine=14,title=deburr clone comparison::clone clone-1 family=family-1 state=matched identity_ambiguous=true functions=pkg/a/one.go::One",
            "::notice file=b/two.go,line=20,endLine=31,title=deburr clone comparison::clone clone-1 family=family-1 state=matched identity_ambiguous=true functions=pkg/b/two.go::Two,pkg/b/two.go::Three",
            "1 ambiguous finding matches; no new or resolved claim made",
        ],
    );
}

#[test]
fn github_comparison_flags_unavailable_conclusions_and_duplication_gaps() {
    let mut comparison = sample_comparison();
    comparison.compatible = false;
    comparison.reason = "scopes differ".to_owned();
    comparison.duplication.comparable = false;
    comparison.duplication.reason = "duplication settings differ".to_owned();
    let output = comparison_in(&comparison, Format::Github);

    assert_shows(
        Format::Github,
        &output,
        &[
            "::error title=deburr comparison::scopes differ",
            "title=deburr duplication::before=measured,after=measured,duplication settings differ",
        ],
    );
}
