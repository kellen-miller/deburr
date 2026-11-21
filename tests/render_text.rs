//! Text renderer tests over constructed reports and comparisons.

mod render_fixture;

use deburr::render::{self, Format, Source, Style};
use render_fixture::{
    FailingWriter, comparison_in, report_in, sample_comparison, sample_report,
    sample_report_with_functions,
};

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
fn text_shows_every_report_fact() {
    let output = report_in(&sample_report(), Format::Text);

    assert_shows(
        Format::Text,
        &output,
        &[
            "schema 3",
            "Analyzer: deburr 1.0.0",
            "source_commit=abc123",
            "source_dirty=true",
            "source_manifest_digest=sha256:manifest",
            "build_version=1.0.0-test",
            "Language: go,typescript",
            "Scope: go,typescript",
            "discovered=7 analyzed=5 excluded=1 unsupported=2 read_errors=3 parse_errors=1 symlinks=4 excluded_directories=6",
            "All metrics: code_lines=40 functions=2 cyclomatic=14 max_nesting=2 mass=28.500000",
            "high_complexity_mass_share=0.250000",
            "high_complexity_mass_share=null",
            "Source failures (1)",
            "broken,file.go [parse_error] syntax error at 3:5",
            "Production hotspots (2 of 2)",
            "main.go:1:1-5:2 <script>alert(1)</script> [production] sloc=4 cyclomatic=12 nesting=2 mass=24.000000 high_complexity=true flat_guards=1 field_mappings=2 nested_branches=3 assertion_like_calls=4",
            "Test hotspots (0 of 0)",
            "Findings (1)",
            "main.go:4:2-9:3 [review] duplicate-branches: if branches have identical syntax",
            "owner=F01, candidate_only=true",
            "Duplication: measured min_tokens=80 min_lines=8",
            "production: 25.00% duplication (10 of 40 code lines across 2 files)",
            "clone clone-1 family=family-1 category=production tokens=95 ambiguous=true",
            "a/one.go:3:1-14:2 code_lines=10 functions=pkg/a/one.go::One",
            "b/two.go:20:1-31:2 code_lines=9 functions=pkg/b/two.go::Two,pkg/b/two.go::Three",
            "Review: total=3 unreviewed=2 refactored=0 retained=1 deferred=0 stale=1",
            "- finding finding-1 [retained]",
            "reason_category=cohesive-validation reason=branches are intentionally parallel evidence=read tests upstream=owner-team",
            "- function pkg/main.go::<script> [unreviewed] locations=main.go:1:1-5:2 stale=true",
            "previous=deferred; reason_category=upstream; reason=generated contract; evidence=schema owner; upstream=schema-team; fingerprint=sha256:old",
        ],
    );
}

#[test]
fn text_limits_hotspots_but_reports_the_remainder() {
    let report = sample_report_with_functions(25);
    let output = report_in(&report, Format::Text);

    assert_shows(
        Format::Text,
        &output,
        &[
            "Production hotspots (20 of 25)",
            "... 5 more; use JSON/HTML for the full inventory",
            "include all 25",
        ],
    );

    let first = output.find("F24").expect("heaviest function listed");
    let second = output.find("F23").expect("next function listed");
    assert!(first < second, "hotspots are ranked by mass:\n{output}");
    assert!(!output.contains("F04 "), "lightest functions are cut off");
}

#[test]
fn text_reports_not_requested_duplication_and_empty_scans() {
    let mut report = sample_report();
    report.duplication.status = deburr::report::DuplicationStatus::NotRequested;
    report.duplication.categories.clear();
    report.duplication.clones.clear();
    report.coverage.analyzed = 0;
    let output = report_in(&report, Format::Text);

    assert_shows(
        Format::Text,
        &output,
        &[
            "Duplication: not_requested",
            "no analyzable Go or TypeScript files",
        ],
    );
}

#[test]
fn text_comparison_shows_every_fact() {
    let output = comparison_in(&sample_comparison(), Format::Text);

    assert_shows(
        Format::Text,
        &output,
        &[
            "Deburr comparison (schema 3)",
            "Analyzer: deburr 1.0.0 -> deburr 1.0.0",
            "Before provenance: build_version=1.0.0-test source_commit=abc123",
            "After provenance: build_version=1.0.1-test source_commit=def456",
            "Compatible: true",
            "Complete: true",
            "Coverage: before analyzed=5 read_errors=3 parse_errors=1; after analyzed=5",
            "All metric delta: code_lines=-5 functions=-1 cyclomatic=-9 max_nesting=-1 mass=-21.500000 high_complexity_functions=-1 high_complexity_mass=-24.000000 high_complexity_mass_share=-0.250000",
            "Review before: total=3 unreviewed=2 refactored=0 retained=1 deferred=0 stale=1",
            "Review after: total=3",
            "New findings (0)",
            "No longer reported findings (1)",
            "Finding changes (1)",
            "- no_longer_reported before=main.go:4:2-9:3 [review] duplicate-branches",
            "Ambiguous finding matches (1)",
            "dup.go:duplicate-branches: two candidates before=b1,b2 after=a1",
            "Function changes (1)",
            "- resolved_threshold match=id before=main.go:1:1-5:2 name=F00",
            "high_complexity=true",
            "after=main.go:1:1-5:2 name=Resolved<b>",
            "before_ids=pkg/main.go::F00 after_ids=pkg/main.go::F00",
            "before_review=retained reason_category=lifecycle-ordering reason=ordering matters evidence=ran tests previous=deferred; reason_category=upstream",
            "note=no_behavior_conclusion",
            "Clone changes (1)",
            "- matched match=fingerprint before=id=clone-1 family=family-1 tokens=95 ambiguous=true endpoints=a/one.go:3:1-14:2 code_lines=10 functions=pkg/a/one.go::One | b/two.go:20:1-31:2 code_lines=9 functions=pkg/b/two.go::Two,pkg/b/two.go::Three",
            "Duplication: before=measured after=measured comparable=true before_config=true/80/8 after_config=true/80/8",
            "- before clone id=clone-1",
        ],
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
