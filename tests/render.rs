//! Renderer tests over constructed reports: every fact a report or comparison
//! holds must appear in every human-facing format.

use std::io::{self, Write};

use deburr::render::{self, Format, github};
use deburr::report::{
    AmbiguousMatch, Change, ChangeState, Comparison, Finding, Function, MatchBasis, MetricDelta,
    MetricsDelta, Report, ReviewDecision, ReviewItem, ReviewKind, ReviewStatus,
};
use serde_json::json;

struct FailingWriter;

impl Write for FailingWriter {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("write failed"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn report_in(report: &Report, format: Format) -> String {
    let mut out = Vec::new();
    render::render_report(report, format, &mut out).expect("render report");

    String::from_utf8(out).expect("utf-8 output")
}

fn comparison_in(comparison: &Comparison, format: Format) -> String {
    let mut out = Vec::new();
    render::render_comparison(comparison, format, &mut out).expect("render comparison");

    String::from_utf8(out).expect("utf-8 output")
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

fn function(index: u32, mass: f64, high_complexity: bool) -> Function {
    serde_json::from_value(json!({
        "id": format!("pkg/main.go::F{index:02}"),
        "fingerprint": format!("sha256:fn{index:02}"),
        "path": "main.go",
        "category": "production",
        "name": format!("F{index:02}"),
        "structural_signals": {
            "flat_guards": 1, "field_mappings": 2, "nested_branches": 3, "assertion_like_calls": 4,
            "clone_like_calls": 5, "unwrap_like_calls": 6, "panic_macros": 7, "unsafe_blocks": 8
        },
        "start": { "line": index + 1, "column": 1 },
        "end": { "line": index + 5, "column": 2 },
        "sloc": 4,
        "cyclomatic": if high_complexity { 12 } else { 2 },
        "max_nesting": 2,
        "mass": mass,
        "high_complexity": high_complexity,
    }))
    .expect("function")
}

/// A report that exercises every section: a hotspot with a hostile name, a
/// finding with details, a parse failure, a measured clone, and a ledger with
/// a retained decision and a stale item.
fn sample_report() -> Report {
    let mut hostile = function(0, 24.0, true);
    hostile.name = "<script>alert(1)</script>".to_owned();
    hostile.id = "pkg/main.go::<script>".to_owned();

    let mut report: Report = serde_json::from_value(json!({
        "duplication": {
            "status": "measured",
            "min_tokens": 80,
            "min_lines": 8,
            "categories": [
                { "category": "production", "files": 2, "code_lines": 40,
                  "duplicated_code_lines": 10, "percentage": 25.0 }
            ],
            "clones": [{
                "id": "clone-1", "family_id": "family-1", "fingerprint": "sha256:clone",
                "category": "production", "tokens": 95, "identity_ambiguous": true,
                "endpoints": [
                    { "path": "a/one.go", "start": { "line": 3, "column": 1 },
                      "end": { "line": 14, "column": 2 }, "code_lines": 10,
                      "functions": ["pkg/a/one.go::One"] },
                    { "path": "b/two.go", "start": { "line": 20, "column": 1 },
                      "end": { "line": 31, "column": 2 }, "code_lines": 9,
                      "functions": ["pkg/b/two.go::Two", "pkg/b/two.go::Three"] }
                ]
            }]
        },
        "provenance": {
            "source": { "commit": "abc123", "manifest_digest": "sha256:manifest",
                        "dirty": true, "git_known": true },
            "build": { "version": "1.0.0-test" }
        },
        "analyzer": { "id": "deburr", "version": "1.0.0" },
        "schema_version": "4",
        "files": [
            { "path": "main.go", "category": "production", "status": "analyzed",
              "bytes": 120, "lines": 12, "code_lines": 9, "inline_test_code_lines": 0 },
            { "path": "broken,file.go", "category": "production", "status": "parse_error",
              "error": "syntax error at 3:5", "bytes": 10, "lines": 2, "code_lines": 1,
              "inline_test_code_lines": 0 }
        ],
        "functions": [],
        "findings": [{
            "id": "finding-1", "fingerprint": "finding-fp", "rule": "duplicate-branches",
            "path": "main.go", "severity": "review",
            "message": "if branches have identical syntax; review whether the condition and branch effects can be simplified",
            "details": [{ "key": "owner", "value": "F01" }, { "key": "candidate_only", "value": "true" }],
            "start": { "line": 4, "column": 2 }, "end": { "line": 9, "column": 3 }
        }],
        "scope": { "path_mode": "source-relative", "languages": ["go", "typescript"],
                   "include_tests": true, "include_testdata": false,
                   "include_vendor": false, "include_generated": false },
        "metrics": {
            "all": { "high_complexity_mass_share": 0.25, "code_lines": 40, "functions": 2,
                     "cyclomatic": 14, "max_nesting": 2, "mass": 28.5,
                     "high_complexity_functions": 1, "high_complexity_mass": 24.0 },
            "production": { "high_complexity_mass_share": null, "code_lines": 40, "functions": 2,
                            "cyclomatic": 14, "max_nesting": 2, "mass": 28.5,
                            "high_complexity_functions": 1, "high_complexity_mass": 24.0 },
            "test": { "high_complexity_mass_share": null, "code_lines": 0, "functions": 0,
                      "cyclomatic": 0, "max_nesting": 0, "mass": 0.0,
                      "high_complexity_functions": 0, "high_complexity_mass": 0.0 }
        },
        "config": {
            "rules": [{ "id": "duplicate-branches", "enabled": true }],
            "language": "go,typescript",
            "duplication": { "requested": true, "min_tokens": 80, "min_lines": 8 },
            "high_complexity_threshold": 10,
            "include_tests": true, "include_testdata": false,
            "include_vendor": false, "include_generated": false
        },
        "coverage": {
            "by_category": [],
            "discovered": 7, "analyzed": 5, "excluded": 1, "unsupported": 2,
            "read_errors": 3, "parse_errors": 1, "symlinks": 4, "excluded_directories": 6
        }
    }))
    .expect("report");

    report.functions = vec![hostile, function(1, 4.5, false)];
    report.review = Some(ledger());

    report
}

fn ledger() -> deburr::report::ReviewLedger {
    serde_json::from_value(json!({
        "schema_version": "1",
        "report_identity": "sha256:identity",
        "source_manifest_digest": "sha256:manifest",
        "items": [
            { "id": "clone-1", "kind": "clone", "fingerprint": "sha256:clone",
              "status": "unreviewed", "locations": [
                  { "path": "a/one.go", "start": { "line": 3, "column": 1 }, "end": { "line": 14, "column": 2 } },
                  { "path": "b/two.go", "start": { "line": 20, "column": 1 }, "end": { "line": 31, "column": 2 } }
              ] },
            { "id": "finding-1", "kind": "finding", "fingerprint": "finding-fp",
              "status": "retained", "reason_category": "cohesive-validation",
              "reason": "branches are intentionally parallel", "evidence": "read tests",
              "upstream": "owner-team", "locations": [
                  { "path": "main.go", "start": { "line": 4, "column": 2 }, "end": { "line": 9, "column": 3 } }
              ] },
            { "id": "pkg/main.go::<script>", "kind": "function", "fingerprint": "sha256:fn00",
              "status": "unreviewed", "stale": true,
              "previous": { "fingerprint": "sha256:old", "status": "deferred",
                            "reason_category": "upstream", "reason": "generated contract",
                            "evidence": "schema owner", "upstream": "schema-team" },
              "locations": [
                  { "path": "main.go", "start": { "line": 1, "column": 1 }, "end": { "line": 5, "column": 2 } }
              ] }
        ],
        "counts": { "total": 3, "unreviewed": 2, "refactored": 0, "retained": 1,
                    "deferred": 0, "stale": 1 }
    }))
    .expect("ledger")
}

#[test]
fn text_shows_every_report_fact() {
    let output = report_in(&sample_report(), Format::Text);

    assert_shows(
        Format::Text,
        &output,
        &[
            "schema 4",
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
            "main.go:1:1-5:2 <script>alert(1)</script> [production] sloc=4 cyclomatic=12 nesting=2 mass=24.000000 high_complexity=true flat_guards=1 field_mappings=2 nested_branches=3 assertion_like_calls=4 clone_like_calls=5 unwrap_like_calls=6 panic_macros=7 unsafe_blocks=8",
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
    let mut report = sample_report();
    report.functions = (0..25)
        .map(|index| function(index, f64::from(index), false))
        .collect();
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
            "Schema <code>4</code>",
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
            "sloc=4 cyclomatic=12 max_nesting=2 mass=24.000000 flat_guards=1 field_mappings=2 nested_branches=3 assertion_like_calls=4 clone_like_calls=5 unwrap_like_calls=6 panic_macros=7 unsafe_blocks=8",
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
    let mut report = sample_report();
    report.functions = (0..25)
        .map(|index| function(index, f64::from(index), false))
        .collect();
    let output = report_in(&report, Format::Html);

    let heaviest = output.find("pkg/main.go::F24").expect("F24 present");
    let lightest = output.find("pkg/main.go::F00").expect("F00 present");
    assert!(heaviest < lightest, "inventory is ranked by mass");
    assert_shows(Format::Html, &output, &["Function inventory (25)"]);
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
            "::notice file=main.go,line=1,endLine=5,title=deburr function::<script>alert(1)</script> sloc=4 cyclomatic=12 nesting=2 mass=24.000000 flat_guards=1 field_mappings=2 nested_branches=3 assertion_like_calls=4 clone_like_calls=5 unwrap_like_calls=6 panic_macros=7 unsafe_blocks=8 identity_ambiguous=false",
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
fn json_round_trips_the_report() {
    let report = sample_report();
    let output = report_in(&report, Format::Json);
    let decoded: Report = serde_json::from_str(&output).expect("decode");

    assert_eq!(decoded, report);
    assert!(output.ends_with('\n'));
}

#[test]
fn every_format_reports_writer_errors() {
    let report = sample_report();
    for format in [Format::Text, Format::Json, Format::Html, Format::Github] {
        assert!(
            render::render_report(&report, format, &mut FailingWriter).is_err(),
            "{} swallowed a write error",
            format.name()
        );
    }
}

#[test]
fn unreviewed_reports_get_a_fresh_ledger() {
    let mut report = sample_report();
    report.review = None;

    assert_shows(
        Format::Text,
        &report_in(&report, Format::Text),
        &["Review: total="],
    );
    assert_shows(
        Format::Github,
        &report_in(&report, Format::Github),
        &["title=deburr review::total="],
    );
    assert_shows(
        Format::Html,
        &report_in(&report, Format::Html),
        &["Primary review items: "],
    );
}

fn sample_comparison() -> Comparison {
    let before = sample_report();
    let mut after = sample_report();
    after.provenance.source.commit = "def456".to_owned();
    after.provenance.build.version = "1.0.1-test".to_owned();

    let mut comparison = deburr::compare::compare(&before, &after);
    comparison.compatible = true;
    comparison.complete = true;
    comparison.reason = String::new();
    // Renderers show the ledgers they are given; fix them here instead of
    // depending on how `compare` derives ledgers from the sample reports.
    comparison.before_review = ledger();
    comparison.after_review = ledger();
    comparison.metrics_delta = Some(MetricsDelta {
        all: MetricDelta {
            high_complexity_mass_share: Some(-0.25),
            code_lines: -5,
            functions: -1,
            cyclomatic: -9,
            max_nesting: -1,
            mass: -21.5,
            high_complexity_functions: -1,
            high_complexity_mass: -24.0,
        },
        ..MetricsDelta::default()
    });

    let mut resolved = function(0, 3.0, false);
    resolved.name = "Resolved<b>".to_owned();
    let hotspot = function(0, 24.0, true);
    let retained = ReviewItem {
        previous: Some(ReviewDecision {
            fingerprint: "sha256:old".to_owned(),
            status: ReviewStatus::Deferred,
            reason_category: "upstream".to_owned(),
            reason: "generated contract".to_owned(),
            evidence: "schema owner".to_owned(),
            upstream: "schema-team".to_owned(),
        }),
        id: hotspot.id.clone(),
        kind: ReviewKind::Function,
        fingerprint: "sha256:fn00".to_owned(),
        status: ReviewStatus::Retained,
        reason_category: "lifecycle-ordering".to_owned(),
        reason: "ordering matters".to_owned(),
        evidence: "ran tests".to_owned(),
        upstream: String::new(),
        locations: Vec::new(),
        stale: false,
        identity_ambiguous: false,
    };
    comparison.function_changes = vec![Change {
        state: ChangeState::ResolvedThreshold,
        match_basis: Some(MatchBasis::Id),
        before: Some(hotspot),
        after: Some(resolved),
        before_review: Some(retained),
        after_review: None,
        before_ids: vec!["pkg/main.go::F00".to_owned()],
        after_ids: vec!["pkg/main.go::F00".to_owned()],
    }];

    let finding = before.findings[0].clone();
    comparison.new_findings = Vec::new();
    comparison.resolved_findings = vec![finding.clone()];
    comparison.finding_changes = vec![Change {
        state: ChangeState::NoLongerReported,
        match_basis: None,
        before: Some(finding),
        after: None,
        before_review: None,
        after_review: None,
        before_ids: Vec::new(),
        after_ids: Vec::new(),
    }];

    let clone = before.duplication.clones[0].clone();
    comparison.clone_changes = vec![Change {
        state: ChangeState::Matched,
        match_basis: Some(MatchBasis::Fingerprint),
        before: Some(clone.clone()),
        after: Some(clone.clone()),
        before_review: None,
        after_review: None,
        before_ids: Vec::new(),
        after_ids: Vec::new(),
    }];
    comparison.duplication.comparable = true;
    comparison.duplication.before_clones = vec![clone];
    comparison.ambiguous_findings = vec![AmbiguousMatch {
        kind: "finding".to_owned(),
        state: Some(ChangeState::Ambiguous),
        match_basis: None,
        rule: "duplicate-branches".to_owned(),
        path: "dup.go".to_owned(),
        message: "two candidates".to_owned(),
        before_ids: vec!["b1".to_owned(), "b2".to_owned()],
        after_ids: vec!["a1".to_owned()],
    }];

    comparison
}

#[test]
fn text_comparison_shows_every_fact() {
    let output = comparison_in(&sample_comparison(), Format::Text);

    assert_shows(
        Format::Text,
        &output,
        &[
            "Deburr comparison (schema 4)",
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
fn text_comparison_refuses_to_conclude_when_incompatible() {
    let mut comparison = sample_comparison();
    comparison.compatible = false;
    comparison.metrics_delta = None;
    comparison.reason = "analyzer versions differ".to_owned();

    for format in [Format::Text, Format::Html, Format::Github] {
        let output = comparison_in(&comparison, format);
        assert_shows(format, &output, &["analyzer versions differ"]);
    }

    let text = comparison_in(&comparison, Format::Text);
    assert_shows(
        Format::Text,
        &text,
        &[
            "Compatible: false",
            "Change conclusion: unavailable",
            "Metrics delta: unavailable",
        ],
    );
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
            "Schema <code>4</code>",
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

#[test]
fn json_comparison_round_trips() {
    let comparison = sample_comparison();
    let output = comparison_in(&comparison, Format::Json);
    let decoded: Comparison = serde_json::from_str(&output).expect("decode");

    assert_eq!(decoded, comparison);
}

#[test]
fn every_format_reports_comparison_writer_errors() {
    let comparison = sample_comparison();
    for format in [Format::Text, Format::Json, Format::Html, Format::Github] {
        assert!(
            render::render_comparison(&comparison, format, &mut FailingWriter).is_err(),
            "{} swallowed a write error",
            format.name()
        );
    }
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
