//! Report comparison semantics, ported from the Go `compare_test.go`
//! scenarios and extended for clones and duplication settings.

use deburr::compare::compare;
use deburr::report::{
    Category, ChangeState, CloneEndpoint, CodeClone, DuplicationStatus, File, FileStatus, Function,
    MatchBasis, Position, Report, ReviewKind, ReviewStatus, StructuralSignals,
};
use deburr::review;
use serde_json::json;

fn bucket(code_lines: u32, mass: f64, share: Option<f64>) -> serde_json::Value {
    json!({
        "high_complexity_mass_share": share,
        "code_lines": code_lines,
        "functions": 1,
        "cyclomatic": 2,
        "max_nesting": 0,
        "mass": mass,
        "high_complexity_functions": 0,
        "high_complexity_mass": 0.0,
    })
}

/// One finding in `main.go`, measured duplication with no clones, and
/// coverage of one analyzed file.
fn sample_report() -> Report {
    serde_json::from_value(json!({
        "duplication": {
            "status": "measured",
            "min_tokens": 80,
            "min_lines": 8,
            "categories": [],
            "clones": [],
        },
        "provenance": {
            "source": {"commit": "", "manifest_digest": "", "dirty": false, "git_known": false},
            "build": {"version": "1.0.0"},
        },
        "analyzer": {"id": "deburr", "version": "1.0.0"},
        "schema_version": "3",
        "files": [],
        "functions": [],
        "findings": [{
            "id": "finding-1",
            "rule": "style",
            "path": "main.go",
            "severity": "warning",
            "message": "simplify return",
            "start": {"line": 2, "column": 1},
            "end": {"line": 2, "column": 2},
        }],
        "scope": {
            "path_mode": "source-relative",
            "languages": ["go"],
            "include_tests": true,
            "include_testdata": false,
            "include_vendor": false,
            "include_generated": false,
        },
        "metrics": {
            "all": bucket(10, 4.0, Some(0.2)),
            "production": bucket(0, 0.0, None),
            "test": bucket(0, 0.0, None),
        },
        "config": {
            "rules": [],
            "language": "go",
            "duplication": {"requested": true, "min_tokens": 80, "min_lines": 8},
            "high_complexity_threshold": 10,
            "include_tests": true,
            "include_testdata": false,
            "include_vendor": false,
            "include_generated": false,
        },
        "coverage": {
            "by_category": [],
            "discovered": 1,
            "analyzed": 1,
            "excluded": 0,
            "unsupported": 0,
            "read_errors": 0,
            "parse_errors": 0,
            "symlinks": 0,
            "excluded_directories": 0,
        },
    }))
    .expect("sample report")
}

fn function(id: &str, fingerprint: &str, name: &str, high_complexity: bool) -> Function {
    Function {
        id: id.to_owned(),
        fingerprint: fingerprint.to_owned(),
        path: "main.go".to_owned(),
        category: Category::Production,
        name: name.to_owned(),
        structural_signals: StructuralSignals::default(),
        start: Position { line: 1, column: 1 },
        end: Position { line: 9, column: 1 },
        sloc: 8,
        cyclomatic: if high_complexity { 11 } else { 2 },
        max_nesting: 1,
        mass: if high_complexity { 12.0 } else { 2.0 },
        high_complexity,
        identity_ambiguous: false,
    }
}

fn clone_of(id: &str, fingerprint: &str, path: &str, line: u32) -> CodeClone {
    let endpoint = |path: &str, line: u32| CloneEndpoint {
        path: path.to_owned(),
        start: Position { line, column: 1 },
        end: Position {
            line: line + 7,
            column: 2,
        },
        code_lines: 8,
        functions: Vec::new(),
    };

    CodeClone {
        id: id.to_owned(),
        family_id: "family".to_owned(),
        fingerprint: fingerprint.to_owned(),
        category: Category::Production,
        tokens: 80,
        identity_ambiguous: false,
        endpoints: [endpoint(path, line), endpoint("other.go", 1)],
    }
}

#[test]
fn compatible_reports_carry_metric_deltas_and_ignore_line_moves() {
    let before = sample_report();
    let mut after = sample_report();
    after.metrics.all.code_lines = 12;
    after.metrics.all.mass = 5.5;
    after.metrics.all.high_complexity_mass_share = Some(0.5);
    after.findings[0].start.line = 20;

    let result = compare(&before, &after);
    let delta = result.metrics_delta.expect("metrics delta");

    assert!(result.compatible && result.complete);
    assert!(result.reason.is_empty());
    assert_eq!(delta.all.code_lines, 2);
    assert!((delta.all.mass - 1.5).abs() < 1e-12);
    assert!((delta.all.high_complexity_mass_share.expect("share") - 0.3).abs() < 1e-12);
    assert_eq!(delta.production.high_complexity_mass_share, None);
    assert!(result.new_findings.is_empty() && result.resolved_findings.is_empty());
    assert_eq!(result.finding_changes.len(), 1);
    assert_eq!(result.finding_changes[0].state, ChangeState::Matched);
    assert_eq!(result.finding_changes[0].match_basis, Some(MatchBasis::Id));
}

#[test]
fn configuration_change_is_incompatible_without_findings_or_delta() {
    let before = sample_report();
    let mut after = sample_report();
    after.config.high_complexity_threshold = 99;
    after.findings.clear();

    let result = compare(&before, &after);

    assert!(!result.compatible && !result.complete);
    assert_eq!(result.reason, "configuration differs");
    assert!(result.metrics_delta.is_none());
    assert!(result.finding_changes.is_empty());
    assert!(result.new_findings.is_empty() && result.resolved_findings.is_empty());
}

#[test]
fn scope_schema_and_analyzer_differences_are_incompatible_in_fixed_order() {
    let before = sample_report();

    let mut scope = sample_report();
    scope.scope.include_vendor = true;
    assert_eq!(compare(&before, &scope).reason, "analysis scope differs");

    let mut analyzer = sample_report();
    analyzer.analyzer.version = "2.0.0".to_owned();
    assert_eq!(
        compare(&before, &analyzer).reason,
        "analyzer identity differs"
    );

    let mut schema = sample_report();
    schema.schema_version = "4".to_owned();
    assert_eq!(
        compare(&before, &schema).reason,
        "schema versions differ: 3 vs 4"
    );

    // Everything differs; the first rule in the fixed order wins.
    let mut everything = sample_report();
    everything.schema_version = "4".to_owned();
    everything.analyzer.version = "2.0.0".to_owned();
    everything.scope.include_vendor = true;
    assert_eq!(
        compare(&before, &everything).reason,
        "schema versions differ: 3 vs 4"
    );
}

#[test]
fn incomplete_coverage_cannot_claim_improvement() {
    for read_errors in [0, 1] {
        let before = sample_report();
        let mut after = sample_report();
        after.coverage.read_errors = read_errors;
        after.coverage.parse_errors = 1 - read_errors;
        after.findings.clear();

        let result = compare(&before, &after);

        assert!(!result.compatible && !result.complete);
        assert!(result.reason.starts_with("coverage is incomplete"));
        assert!(result.metrics_delta.is_none());
    }

    let before = sample_report();
    let mut after = sample_report();
    after.coverage.analyzed = 0;

    assert!(!compare(&before, &after).complete);
}

#[test]
fn duplicate_finding_ids_are_ambiguous_not_new_or_resolved() {
    let mut before = sample_report();
    let mut after = sample_report();
    before.findings.push(before.findings[0].clone());
    after.findings.push(after.findings[0].clone());

    let result = compare(&before, &after);

    assert_eq!(result.ambiguous_findings.len(), 1);
    assert_eq!(result.finding_changes.len(), 1);
    assert_eq!(result.finding_changes[0].state, ChangeState::Ambiguous);
    assert_eq!(
        result.finding_changes[0].before_ids,
        ["finding-1", "finding-1"]
    );
    assert_eq!(result.ambiguous_findings[0].kind, "finding");
    assert_eq!(result.ambiguous_findings[0].rule, "style");
    assert_eq!(result.ambiguous_findings[0].path, "main.go");
    assert!(result.new_findings.is_empty() && result.resolved_findings.is_empty());
}

#[test]
fn identity_ambiguous_finding_cannot_match_by_ordinal_id() {
    let mut before = sample_report();
    before.findings[0].fingerprint = "same-source".to_owned();
    before.findings[0].identity_ambiguous = true;
    let mut after = before.clone();
    after.findings[0].identity_ambiguous = false;

    let result = compare(&before, &after);

    assert_eq!(result.ambiguous_findings.len(), 1);
    assert_eq!(result.finding_changes.len(), 1);
    assert_eq!(result.finding_changes[0].state, ChangeState::Ambiguous);
    assert_eq!(result.finding_changes[0].match_basis, Some(MatchBasis::Id));
    assert!(result.finding_changes[0].before.is_some());
    assert!(result.finding_changes[0].after.is_some());
    assert!(result.new_findings.is_empty() && result.resolved_findings.is_empty());
}

#[test]
fn unique_fingerprint_matches_after_a_move() {
    let mut before = sample_report();
    before.findings[0].id = "old-path-id".to_owned();
    before.findings[0].fingerprint = "same-source".to_owned();
    let mut after = sample_report();
    after.findings[0].id = "new-path-id".to_owned();
    after.findings[0].path = "moved.go".to_owned();
    after.findings[0].fingerprint = "same-source".to_owned();

    let result = compare(&before, &after);

    assert_eq!(result.finding_changes.len(), 1);
    assert_eq!(result.finding_changes[0].state, ChangeState::Matched);
    assert_eq!(
        result.finding_changes[0].match_basis,
        Some(MatchBasis::Fingerprint)
    );
    assert!(result.new_findings.is_empty() && result.resolved_findings.is_empty());
}

#[test]
fn id_pass_wins_before_fingerprints_are_considered() {
    let mut before = sample_report();
    before.findings[0].fingerprint = "a".to_owned();
    let mut second = before.findings[0].clone();
    second.id = "finding-2".to_owned();
    second.fingerprint = "b".to_owned();
    before.findings.push(second);

    let mut after = before.clone();
    // Same IDs, swapped fingerprints: IDs decide, so both stay matched by id.
    after.findings[0].fingerprint = "b".to_owned();
    after.findings[1].fingerprint = "a".to_owned();

    let result = compare(&before, &after);

    assert_eq!(result.finding_changes.len(), 2);
    assert!(
        result
            .finding_changes
            .iter()
            .all(|change| change.match_basis == Some(MatchBasis::Id))
    );
}

#[test]
fn repeated_fingerprint_without_stable_ids_is_ambiguous() {
    let finding = |message: &str| {
        let mut finding = sample_report().findings.remove(0);
        finding.id = String::new();
        finding.fingerprint = "same".to_owned();
        finding.message = message.to_owned();
        finding
    };
    let mut before = sample_report();
    before.findings = vec![finding("one"), finding("two")];
    let mut after = sample_report();
    after.findings = vec![finding("one after"), finding("two after")];

    let result = compare(&before, &after);

    assert_eq!(result.ambiguous_findings.len(), 1);
    assert_eq!(result.finding_changes.len(), 1);
    assert_eq!(result.finding_changes[0].state, ChangeState::Ambiguous);
    assert_eq!(
        result.finding_changes[0].match_basis,
        Some(MatchBasis::Fingerprint)
    );
    assert!(result.new_findings.is_empty() && result.resolved_findings.is_empty());
}

#[test]
fn new_and_resolved_findings_are_aggregated() {
    let before = sample_report();
    let mut after = sample_report();
    after.findings[0].id = "finding-2".to_owned();
    after.findings[0].fingerprint = "other".to_owned();

    let result = compare(&before, &after);
    let states: Vec<ChangeState> = result.finding_changes.iter().map(|c| c.state).collect();

    assert_eq!(states.len(), 2);
    assert!(states.contains(&ChangeState::NoLongerReported));
    assert!(states.contains(&ChangeState::New));
    assert_eq!(result.new_findings.len(), 1);
    assert_eq!(result.new_findings[0].id, "finding-2");
    assert_eq!(result.resolved_findings.len(), 1);
    assert_eq!(result.resolved_findings[0].id, "finding-1");
}

#[test]
fn finding_in_a_removed_file_is_source_removed() {
    let mut before = sample_report();
    before.files.push(analyzed_file("main.go"));
    let mut after = sample_report();
    after.files.push(analyzed_file("other.go"));
    after.findings.clear();

    let result = compare(&before, &after);

    assert_eq!(result.finding_changes.len(), 1);
    assert_eq!(result.finding_changes[0].state, ChangeState::SourceRemoved);
    assert_eq!(result.resolved_findings.len(), 1);

    // With the file still present the finding is merely no longer reported.
    let mut still_there = sample_report();
    still_there.files.push(analyzed_file("main.go"));
    still_there.findings.clear();

    assert_eq!(
        compare(&before, &still_there).finding_changes[0].state,
        ChangeState::NoLongerReported
    );
}

fn analyzed_file(path: &str) -> File {
    File {
        path: path.to_owned(),
        category: Category::Production,
        status: FileStatus::Analyzed,
        reason: String::new(),
        error: String::new(),
        bytes: 1,
        lines: 1,
        code_lines: 1,
    }
}

#[test]
fn function_threshold_transitions_and_full_inventory() {
    let mut before = sample_report();
    before.functions = vec![
        function("hot", "hot-source", "Hot", true),
        function("small", "small-source", "Small", false),
        function("cold", "cold-source", "Cold", false),
    ];
    let mut after = sample_report();
    after.functions = vec![
        function("hot", "hot-source-after", "Hot", false),
        function("small", "small-source", "Small", false),
        function("cold", "cold-source", "Cold", true),
    ];

    let result = compare(&before, &after);
    let state_of = |id: &str| {
        result
            .function_changes
            .iter()
            .find(|change| change.before.as_ref().is_some_and(|f| f.id == id))
            .map(|change| change.state)
    };

    assert_eq!(result.function_changes.len(), 3);
    assert_eq!(state_of("hot"), Some(ChangeState::ResolvedThreshold));
    assert_eq!(state_of("small"), Some(ChangeState::Matched));
    assert_eq!(state_of("cold"), Some(ChangeState::CrossedThreshold));
}

#[test]
fn source_change_with_equal_metrics_stays_matched() {
    let mut before = sample_report();
    before.findings[0].fingerprint = "source-before".to_owned();
    before.provenance.build.version = "build-before".to_owned();
    before.provenance.source.commit = "source-before".to_owned();
    let mut after = before.clone();
    after.findings[0].fingerprint = "source-after".to_owned();
    after.findings[0].message = "same measurement, changed source".to_owned();
    after.provenance.build.version = "build-after".to_owned();
    after.provenance.source.commit = "source-after".to_owned();

    let result = compare(&before, &after);

    assert!(result.compatible && result.complete);
    assert_eq!(result.finding_changes.len(), 1);
    assert_eq!(result.finding_changes[0].state, ChangeState::Matched);
    assert_eq!(result.finding_changes[0].match_basis, Some(MatchBasis::Id));
}

#[test]
fn identity_ambiguous_function_cannot_match_by_ordinal_id() {
    let mut before = sample_report();
    let mut anonymous = function("ordinal", "same", "Anonymous", false);
    anonymous.identity_ambiguous = true;
    before.functions = vec![anonymous];
    let after = before.clone();

    let result = compare(&before, &after);

    assert_eq!(result.ambiguous_functions.len(), 1);
    assert_eq!(result.ambiguous_functions[0].kind, "function");
    assert_eq!(result.ambiguous_functions[0].message, "Anonymous");
    assert_eq!(result.function_changes.len(), 1);
    assert_eq!(result.function_changes[0].state, ChangeState::Ambiguous);
}

#[test]
fn removed_function_is_distinct_from_threshold_resolution() {
    let mut before = sample_report();
    before.files = vec![analyzed_file("main.go")];
    before.functions = vec![function("removed", "removed-source", "Removed", true)];
    let mut after = sample_report();
    after.files = vec![analyzed_file("main.go")];

    let result = compare(&before, &after);

    assert_eq!(result.function_changes.len(), 1);
    assert_eq!(
        result.function_changes[0].state,
        ChangeState::DeletedFunction
    );

    let mut file_gone = sample_report();
    file_gone.files = vec![analyzed_file("elsewhere.go")];
    file_gone.findings.clear();

    assert_eq!(
        compare(&before, &file_gone).function_changes[0].state,
        ChangeState::SourceRemoved
    );
}

#[test]
fn new_function_is_reported_new() {
    let before = sample_report();
    let mut after = sample_report();
    after.functions = vec![function("added", "added-source", "Added", false)];

    let result = compare(&before, &after);

    assert_eq!(result.function_changes.len(), 1);
    assert_eq!(result.function_changes[0].state, ChangeState::New);
    assert!(result.function_changes[0].before.is_none());
}

#[test]
fn clones_match_by_id_then_fingerprint_and_report_additions_and_removals() {
    let mut before = sample_report();
    before.duplication.clones = vec![
        clone_of("clone-a", "fp-a", "a.go", 10),
        clone_of("clone-b", "fp-b", "b.go", 10),
        clone_of("clone-gone", "fp-gone", "gone.go", 10),
    ];
    let mut after = sample_report();
    after.duplication.clones = vec![
        clone_of("clone-a", "fp-a-edited", "a.go", 30),
        clone_of("clone-b-moved", "fp-b", "b.go", 50),
        clone_of("clone-new", "fp-new", "new.go", 10),
    ];

    let result = compare(&before, &after);
    let outcomes: Vec<(ChangeState, Option<MatchBasis>)> = result
        .clone_changes
        .iter()
        .map(|change| (change.state, change.match_basis))
        .collect();

    assert_eq!(result.clone_changes.len(), 4);
    assert!(outcomes.contains(&(ChangeState::Matched, Some(MatchBasis::Id))));
    assert!(outcomes.contains(&(ChangeState::Matched, Some(MatchBasis::Fingerprint))));
    assert!(outcomes.contains(&(ChangeState::Removed, None)));
    assert!(outcomes.contains(&(ChangeState::New, None)));
    assert!(result.ambiguous_clones.is_empty());
}

#[test]
fn identity_ambiguous_clone_is_ambiguous_and_keeps_its_family() {
    let mut before = sample_report();
    let mut clone = clone_of("clone", "clone-source", "a.go", 10);
    clone.identity_ambiguous = true;
    before.duplication.clones = vec![clone];
    let after = before.clone();

    let result = compare(&before, &after);

    assert_eq!(result.ambiguous_clones.len(), 1);
    assert_eq!(result.ambiguous_clones[0].kind, "clone");
    assert_eq!(result.ambiguous_clones[0].message, "duplicate source spans");
    assert_eq!(result.clone_changes.len(), 1);
    assert_eq!(result.clone_changes[0].state, ChangeState::Ambiguous);
    assert_eq!(
        result.clone_changes[0]
            .before
            .as_ref()
            .expect("before")
            .family_id,
        "family"
    );
}

#[test]
fn changes_are_sorted_by_path_then_position() {
    let mut before = sample_report();
    before.functions = vec![
        function("c", "fc", "C", false),
        function("a", "fa", "A", false),
        function("b", "fb", "B", false),
    ];
    before.functions[0].path = "z.go".to_owned();
    before.functions[1].path = "a.go".to_owned();
    before.functions[2].path = "a.go".to_owned();
    before.functions[2].start.line = 30;
    let after = before.clone();

    let result = compare(&before, &after);
    let ids: Vec<&str> = result
        .function_changes
        .iter()
        .map(|change| change.after.as_ref().expect("after").id.as_str())
        .collect();

    assert_eq!(ids, ["a", "b", "c"]);
}

#[test]
fn review_decisions_and_accounting_are_preserved() {
    let mut before = sample_report();
    before.findings[0].fingerprint = "finding-source".to_owned();
    let mut ledger = review::init_ledger(&before);

    for item in &mut ledger.items {
        if item.kind == ReviewKind::Finding {
            item.status = ReviewStatus::Refactored;
            item.reason_category = "cleanup".to_owned();
            item.reason = "simplified branch".to_owned();
            item.evidence = "reviewed source".to_owned();
        }
    }

    before.review = Some(
        review::apply_ledger(&before, &ledger)
            .expect("apply")
            .0
            .review
            .expect("ledger"),
    );
    let mut after = before.clone();
    after.findings.clear();
    after.review = None;

    let result = compare(&before, &after);

    assert_eq!(result.before_review.counts.refactored, 1);
    assert_eq!(result.before_review.counts.total, 1);
    assert_eq!(result.after_review.counts.total, 0);
    assert_eq!(result.finding_changes.len(), 1);
    assert_eq!(
        result.finding_changes[0].state,
        ChangeState::NoLongerReported
    );

    let carried = result.finding_changes[0]
        .before_review
        .as_ref()
        .expect("before review");

    assert_eq!(carried.status, ReviewStatus::Refactored);
    assert_eq!(carried.reason, "simplified branch");
}

#[test]
fn invalid_attached_ledger_refuses_the_comparison() {
    let before = sample_report();
    let mut after = sample_report();
    let mut ledger = review::init_ledger(&after);
    ledger.report_identity = "sha256:forged".to_owned();
    after.review = Some(ledger);

    let result = compare(&before, &after);

    assert!(!result.compatible && !result.complete);
    assert!(result.reason.starts_with("after review ledger is invalid"));
    assert!(result.metrics_delta.is_none());
    assert!(result.finding_changes.is_empty());
}

#[test]
fn requested_duplication_failure_is_incomplete() {
    let before = sample_report();
    let mut after = sample_report();
    after.duplication.status = DuplicationStatus::Error;
    after.duplication.error = "detector failed".to_owned();

    let result = compare(&before, &after);

    assert!(!result.compatible && !result.complete);
    assert!(result.metrics_delta.is_none());
    assert_eq!(
        result.reason,
        "requested duplication measurement is incomplete"
    );
    assert!(!result.duplication.comparable);
    assert_eq!(result.duplication.after_status, DuplicationStatus::Error);
}

#[test]
fn duplication_request_difference_is_incompatible() {
    let before = sample_report();
    let mut after = sample_report();
    after.config.duplication.requested = false;
    after.duplication.status = DuplicationStatus::NotRequested;

    let result = compare(&before, &after);

    assert_eq!(result.reason, "duplication request differs");
    assert!(!result.duplication.comparable);
}

#[test]
fn unrequested_duplication_never_looks_like_zero_clones() {
    let mut before = sample_report();
    before.config.duplication.requested = false;
    before.duplication.status = DuplicationStatus::NotRequested;
    let mut after = before.clone();
    after.duplication.status = DuplicationStatus::Error;

    let result = compare(&before, &after);

    assert!(!result.duplication.comparable);
    assert_eq!(
        result.duplication.before_status,
        DuplicationStatus::NotRequested
    );
    assert_eq!(result.duplication.after_status, DuplicationStatus::Error);
    assert!(!result.compatible);
}

#[test]
fn matching_unrequested_duplication_stays_compatible() {
    let mut before = sample_report();
    before.config.duplication.requested = false;
    before.duplication.status = DuplicationStatus::NotRequested;
    let after = before.clone();

    let result = compare(&before, &after);

    assert!(result.compatible && result.complete);
    assert!(!result.duplication.comparable);
    assert_eq!(
        result.duplication.reason,
        "duplication is comparable only when both reports measured it"
    );
}

#[test]
fn duplication_setting_changes_are_incompatible() {
    let before = sample_report();

    let mut tokens = sample_report();
    tokens.config.duplication.min_tokens = 120;
    tokens.duplication.min_tokens = 120;

    let mut lines = sample_report();
    lines.config.duplication.min_lines = 12;
    lines.duplication.min_lines = 12;

    let mut section_only = sample_report();
    section_only.duplication.min_lines = 12;

    for after in [tokens, lines, section_only] {
        let result = compare(&before, &after);

        assert!(!result.compatible && !result.complete);
        assert!(result.metrics_delta.is_none());
        assert_eq!(result.reason, "duplication settings differ");
        assert!(!result.duplication.comparable);
        assert_eq!(
            result.duplication.reason,
            "duplication configuration differs"
        );
    }
}

#[test]
fn comparable_duplication_carries_both_clone_lists() {
    let mut before = sample_report();
    before.duplication.clones = vec![clone_of("clone-a", "fp-a", "a.go", 10)];
    let after = sample_report();

    let result = compare(&before, &after);

    assert!(result.duplication.comparable);
    assert!(result.duplication.reason.is_empty());
    assert_eq!(result.duplication.before_clones.len(), 1);
    assert!(result.duplication.after_clones.is_empty());
    assert_eq!(result.clone_changes.len(), 1);
    assert_eq!(result.clone_changes[0].state, ChangeState::Removed);
}

#[test]
fn comparison_serializes_to_json_and_back() {
    let mut before = sample_report();
    before.functions = vec![function("hot", "hot-source", "Hot", true)];
    before.duplication.clones = vec![clone_of("clone-a", "fp-a", "a.go", 10)];
    let after = sample_report();

    let result = compare(&before, &after);
    let text = serde_json::to_string(&result).expect("encode");
    let decoded: deburr::report::Comparison = serde_json::from_str(&text).expect("decode");

    assert_eq!(decoded, result);
}
