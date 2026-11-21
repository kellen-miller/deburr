//! Reports and comparisons built by hand for the renderer tests. Each
//! `render_*.rs` test file includes this module and uses all of it.

use std::io::{self, Write};

use deburr::render::{self, Format, Source, Style};
use deburr::report::{
    AmbiguousMatch, Change, ChangeState, Comparison, Function, MatchBasis, MetricDelta,
    MetricsDelta, Report, ReviewDecision, ReviewItem, ReviewKind, ReviewStatus,
};
use serde_json::json;

pub(crate) struct FailingWriter;

impl Write for FailingWriter {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("write failed"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) fn report_in(report: &Report, format: Format) -> String {
    let mut out = Vec::new();
    render::render_report(
        report,
        &Source::default(),
        Style::default(),
        format,
        &mut out,
    )
    .expect("render report");

    String::from_utf8(out).expect("utf-8 output")
}

pub(crate) fn comparison_in(comparison: &Comparison, format: Format) -> String {
    let mut out = Vec::new();
    render::render_comparison(
        comparison,
        &Source::default(),
        Style::default(),
        format,
        &mut out,
    )
    .expect("render comparison");

    String::from_utf8(out).expect("utf-8 output")
}

pub(crate) fn assert_shows(format: Format, output: &str, facts: &[&str]) {
    for fact in facts {
        assert!(
            output.contains(fact),
            "{} output is missing {fact:?}:\n{output}",
            format.name()
        );
    }
}

/// `sample_report` with `count` ordinary functions, `F00` the lightest and
/// `F{count-1}` the heaviest.
pub(crate) fn sample_report_with_functions(count: u32) -> Report {
    let mut report = sample_report();
    report.functions = (0..count)
        .map(|index| function(index, f64::from(index), false))
        .collect();

    report
}

fn function(index: u32, mass: f64, high_complexity: bool) -> Function {
    serde_json::from_value(json!({
        "id": format!("pkg/main.go::F{index:02}"),
        "fingerprint": format!("sha256:fn{index:02}"),
        "path": "main.go",
        "category": "production",
        "name": format!("F{index:02}"),
        "structural_signals": {
            "flat_guards": 1, "field_mappings": 2, "nested_branches": 3, "assertion_like_calls": 4
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
pub(crate) fn sample_report() -> Report {
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
        "schema_version": "3",
        "files": [
            { "path": "main.go", "category": "production", "status": "analyzed",
              "bytes": 120, "lines": 12, "code_lines": 9 },
            { "path": "broken,file.go", "category": "production", "status": "parse_error",
              "error": "syntax error at 3:5", "bytes": 10, "lines": 2, "code_lines": 1 }
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

pub(crate) fn sample_comparison() -> Comparison {
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
