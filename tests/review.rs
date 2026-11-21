//! Review ledger semantics, ported from the Go `review_test.go` scenarios.

use deburr::report::{
    Report, ReviewCounts, ReviewItem, ReviewKind, ReviewLedger, ReviewStatus, ValidationError,
};
use deburr::review::{self, DecisionProblem, ReviewError};
use serde_json::json;

fn clone_json(
    id: &str,
    fingerprint: &str,
    category: &str,
    paths: [&str; 2],
    line: u32,
) -> serde_json::Value {
    let endpoint = |path: &str| {
        json!({
            "path": path,
            "start": {"line": line, "column": 1},
            "end": {"line": line + 7, "column": 2},
            "code_lines": 8,
            "functions": [],
        })
    };

    json!({
        "id": id,
        "family_id": "family",
        "fingerprint": fingerprint,
        "category": category,
        "tokens": 80,
        "endpoints": [endpoint(paths[0]), endpoint(paths[1])],
    })
}

fn bucket() -> serde_json::Value {
    json!({
        "high_complexity_mass_share": null,
        "code_lines": 0,
        "functions": 0,
        "cyclomatic": 0,
        "max_nesting": 0,
        "mass": 0.0,
        "high_complexity_functions": 0,
        "high_complexity_mass": 0.0,
    })
}

fn function_json(
    id: &str,
    name: &str,
    fingerprint: &str,
    high_complexity: bool,
) -> serde_json::Value {
    json!({
        "id": id,
        "fingerprint": fingerprint,
        "path": "main.go",
        "category": "production",
        "name": name,
        "structural_signals": {
            "flat_guards": 0,
            "field_mappings": 0,
            "nested_branches": 0,
            "assertion_like_calls": 0,
            "clone_like_calls": 0,
            "unwrap_like_calls": 0,
            "panic_macros": 0,
            "unsafe_blocks": 0,
        },
        "start": {"line": 10, "column": 1},
        "end": {"line": 40, "column": 1},
        "sloc": 30,
        "cyclomatic": if high_complexity { 11 } else { 1 },
        "max_nesting": 2,
        "mass": 60.0,
        "high_complexity": high_complexity,
    })
}

/// One finding, one high-complexity function, one small function, and two
/// clones (production and test).
fn sample_report() -> Report {
    serde_json::from_value(json!({
        "duplication": {
            "status": "measured",
            "min_tokens": 80,
            "min_lines": 8,
            "categories": [],
            "clones": [
                clone_json("clone-native-1", "clone-source-1", "production", ["main.go", "other.go"], 50),
                clone_json("clone-native-2", "clone-source-2", "test", ["main_test.go", "other_test.go"], 20),
            ],
        },
        "provenance": {
            "source": {"commit": "", "manifest_digest": "", "dirty": false, "git_known": false},
            "build": {"version": "1.0.0"},
        },
        "analyzer": {"id": "deburr", "version": "1.0.0"},
        "schema_version": "4",
        "files": [],
        "functions": [
            function_json("function-native", "Main", "function-source-1", true),
            function_json("function-small", "Small", "", false),
        ],
        "findings": [{
            "id": "finding-native",
            "fingerprint": "finding-source-1",
            "rule": "rule",
            "path": "main.go",
            "severity": "review",
            "message": "candidate",
            "details": [{"key": "owner", "value": "Main"}],
            "start": {"line": 4, "column": 2},
            "end": {"line": 6, "column": 2},
        }],
        "scope": {
            "path_mode": "source-relative",
            "languages": ["go"],
            "include_tests": true,
            "include_testdata": false,
            "include_vendor": false,
            "include_generated": false,
        },
        "metrics": {"all": bucket(), "production": bucket(), "test": bucket()},
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

fn item_of(ledger: &ReviewLedger, kind: ReviewKind) -> &ReviewItem {
    ledger
        .items
        .iter()
        .find(|item| item.kind == kind)
        .expect("item of kind")
}

fn item_of_mut(ledger: &mut ReviewLedger, kind: ReviewKind) -> &mut ReviewItem {
    ledger
        .items
        .iter_mut()
        .find(|item| item.kind == kind)
        .expect("item of kind")
}

fn retain(item: &mut ReviewItem, evidence: &str) {
    item.status = ReviewStatus::Retained;
    item.reason_category = "semantics".to_owned();
    item.reason = "behavior is intentional".to_owned();
    item.evidence = evidence.to_owned();
}

fn ambiguous_function_report(manifest: &str) -> Report {
    let mut report = sample_report();
    report.provenance.source.manifest_digest = manifest.to_owned();
    report.functions[0].identity_ambiguous = true;

    report
}

#[test]
fn init_includes_complete_inventory_in_kind_then_id_order() {
    let ledger = review::init_ledger(&sample_report());

    assert_eq!(
        ledger.counts,
        ReviewCounts {
            total: 4,
            unreviewed: 4,
            ..ReviewCounts::default()
        }
    );

    let kinds: Vec<ReviewKind> = ledger.items.iter().map(|item| item.kind).collect();
    let ids: Vec<&str> = ledger.items.iter().map(|item| item.id.as_str()).collect();

    assert_eq!(
        kinds,
        [
            ReviewKind::Clone,
            ReviewKind::Clone,
            ReviewKind::Finding,
            ReviewKind::Function
        ]
    );
    assert_eq!(
        ids,
        [
            "clone-native-1",
            "clone-native-2",
            "finding-native",
            "function-native"
        ]
    );

    for item in &ledger.items {
        assert!(!item.fingerprint.is_empty());
        assert_eq!(item.status, ReviewStatus::Unreviewed);
        assert!(!item.locations.is_empty());
    }

    assert_eq!(item_of(&ledger, ReviewKind::Clone).locations.len(), 2);
}

#[test]
fn init_omits_functions_below_the_complexity_threshold() {
    let ledger = review::init_ledger(&sample_report());

    assert!(ledger.items.iter().all(|item| item.id != "function-small"));
}

#[test]
fn identity_survives_line_moves() {
    let before = sample_report();
    let mut after = sample_report();
    after.findings[0].start.line += 20;
    after.findings[0].end.line += 20;
    after.functions[0].start.line += 20;
    after.functions[0].end.line += 20;

    for clone in &mut after.duplication.clones {
        for endpoint in &mut clone.endpoints {
            endpoint.start.line += 20;
            endpoint.end.line += 20;
        }
    }

    let before_ledger = review::init_ledger(&before);
    let after_ledger = review::init_ledger(&after);

    assert_eq!(before_ledger.items.len(), after_ledger.items.len());

    for (left, right) in before_ledger.items.iter().zip(&after_ledger.items) {
        assert_eq!(
            (&left.id, &left.fingerprint),
            (&right.id, &right.fingerprint)
        );
    }
}

#[test]
fn apply_accounts_decisions_and_missing_items() {
    let report = sample_report();
    let mut ledger = review::init_ledger(&report);
    ledger.items.truncate(1);
    retain(&mut ledger.items[0], "reviewed source");

    let reviewed = review::apply_ledger(&report, &ledger).expect("apply");
    let applied = reviewed.0.review.expect("attached ledger");

    assert_eq!(
        applied.counts,
        ReviewCounts {
            total: 4,
            unreviewed: 3,
            retained: 1,
            ..ReviewCounts::default()
        }
    );
    assert_eq!(applied.items[0].status, ReviewStatus::Retained);
    assert_eq!(applied.items[0].evidence, "reviewed source");
    assert_eq!(reviewed.0.findings, report.findings);
}

#[test]
fn apply_marks_edited_fingerprint_stale_and_keeps_prior_decision() {
    let mut ledger = review::init_ledger(&sample_report());

    for item in &mut ledger.items {
        item.status = ReviewStatus::Refactored;
        item.reason_category = "change".to_owned();
        item.reason = "simplified".to_owned();
        item.evidence = "reviewed source".to_owned();
    }

    let mut edited = sample_report();
    edited.findings[0].message = "changed candidate".to_owned();
    edited.findings[0].fingerprint = "finding-source-edited".to_owned();

    let applied = review::apply_ledger(&edited, &ledger)
        .expect("apply")
        .0
        .review
        .expect("attached ledger");
    let stale: Vec<&ReviewItem> = applied.items.iter().filter(|item| item.stale).collect();

    assert_eq!(stale.len(), 1);
    assert_eq!(applied.counts.stale, 1);
    assert_eq!(applied.counts.refactored, 3);
    assert_eq!(applied.counts.unreviewed, 1);

    let item = stale[0];
    let previous = item.previous.as_ref().expect("previous decision");

    assert_eq!(item.status, ReviewStatus::Unreviewed);
    assert_eq!(previous.status, ReviewStatus::Refactored);
    assert_eq!(previous.evidence, "reviewed source");
    assert_eq!(previous.fingerprint, "finding-source-1");
}

#[test]
fn stale_item_keeps_the_oldest_previous_decision() {
    let mut ledger = review::init_ledger(&sample_report());
    let finding = item_of_mut(&mut ledger, ReviewKind::Finding);
    retain(finding, "first review");

    let mut edited = sample_report();
    edited.findings[0].fingerprint = "finding-source-2".to_owned();

    let first = review::apply_ledger(&edited, &ledger)
        .expect("first apply")
        .0
        .review
        .expect("ledger");
    let second = review::apply_ledger(&edited, &first)
        .expect("second apply")
        .0
        .review
        .expect("ledger");
    let item = item_of(&second, ReviewKind::Finding);

    assert!(item.stale);
    assert_eq!(
        item.previous.as_ref().expect("previous").evidence,
        "first review"
    );
}

#[test]
fn ambiguous_decision_holds_on_exact_manifest() {
    let report = ambiguous_function_report("sha256:source-a");
    let mut ledger = review::init_ledger(&report);
    retain(
        item_of_mut(&mut ledger, ReviewKind::Function),
        "reviewed exact source snapshot",
    );

    let applied = review::apply_ledger(&report, &ledger)
        .expect("apply")
        .0
        .review
        .expect("ledger");
    let item = item_of(&applied, ReviewKind::Function);

    assert_eq!(item.status, ReviewStatus::Retained);
    assert!(!item.stale);
    assert!(item.previous.is_none());
    assert_eq!(applied.source_manifest_digest, "sha256:source-a");
}

#[test]
fn ambiguous_decision_goes_stale_when_manifest_changes() {
    let baseline = ambiguous_function_report("sha256:source-a");
    let mut ledger = review::init_ledger(&baseline);
    retain(
        item_of_mut(&mut ledger, ReviewKind::Function),
        "reviewed baseline source",
    );

    let changed = ambiguous_function_report("sha256:source-b");
    let applied = review::apply_ledger(&changed, &ledger)
        .expect("apply")
        .0
        .review
        .expect("ledger");
    let item = item_of(&applied, ReviewKind::Function);

    assert_eq!(item.status, ReviewStatus::Unreviewed);
    assert!(item.stale);
    assert_eq!(
        item.previous.as_ref().expect("previous").status,
        ReviewStatus::Retained
    );
    assert_eq!(applied.source_manifest_digest, "sha256:source-b");
}

#[test]
fn ambiguous_decision_goes_stale_when_manifest_is_unknown() {
    let baseline = ambiguous_function_report("sha256:source-a");
    let mut ledger = review::init_ledger(&baseline);
    retain(
        item_of_mut(&mut ledger, ReviewKind::Function),
        "reviewed baseline source",
    );

    let unknown = ambiguous_function_report("");
    let applied = review::apply_ledger(&unknown, &ledger)
        .expect("apply")
        .0
        .review
        .expect("ledger");
    let item = item_of(&applied, ReviewKind::Function);

    assert_eq!(item.status, ReviewStatus::Unreviewed);
    assert!(item.stale);
    assert!(item.previous.is_some());
    assert_eq!(applied.source_manifest_digest, "");
}

#[test]
fn ambiguous_unreviewed_item_is_not_stale_on_manifest_change() {
    let baseline = ambiguous_function_report("sha256:source-a");
    let ledger = review::init_ledger(&baseline);
    let changed = ambiguous_function_report("sha256:source-b");
    let applied = review::apply_ledger(&changed, &ledger)
        .expect("apply")
        .0
        .review
        .expect("ledger");

    assert_eq!(applied.counts.stale, 0);
}

#[test]
fn named_decision_survives_manifest_change() {
    let mut baseline = sample_report();
    baseline.provenance.source.manifest_digest = "sha256:source-a".to_owned();
    let mut ledger = review::init_ledger(&baseline);
    retain(
        item_of_mut(&mut ledger, ReviewKind::Function),
        "reviewed named function",
    );

    let mut changed = sample_report();
    changed.provenance.source.manifest_digest = "sha256:source-b".to_owned();
    let applied = review::apply_ledger(&changed, &ledger)
        .expect("apply")
        .0
        .review
        .expect("ledger");
    let item = item_of(&applied, ReviewKind::Function);

    assert_eq!(item.status, ReviewStatus::Retained);
    assert!(!item.stale);
    assert!(item.previous.is_none());
}

#[test]
fn init_propagates_identity_ambiguity() {
    let mut report = sample_report();
    report.findings[0].identity_ambiguous = true;
    report.functions[0].identity_ambiguous = true;
    report.duplication.clones[0].identity_ambiguous = true;

    let ledger = review::init_ledger(&report);
    let clones: Vec<bool> = ledger
        .items
        .iter()
        .filter(|item| item.kind == ReviewKind::Clone)
        .map(|item| item.identity_ambiguous)
        .collect();

    assert!(item_of(&ledger, ReviewKind::Finding).identity_ambiguous);
    assert!(item_of(&ledger, ReviewKind::Function).identity_ambiguous);
    assert_eq!(clones, [true, false]);
}

#[test]
fn apply_rejects_unknown_id() {
    let report = sample_report();
    let mut ledger = review::init_ledger(&report);
    ledger.items[0].id = "unknown".to_owned();

    assert_eq!(
        review::apply_ledger(&report, &ledger).unwrap_err(),
        ReviewError::UnknownItemId("unknown".to_owned())
    );
}

#[test]
fn apply_rejects_duplicate_ledger_ids() {
    let report = sample_report();
    let mut ledger = review::init_ledger(&report);
    let duplicate = ledger.items[0].clone();
    ledger.items.push(duplicate);

    assert!(matches!(
        review::apply_ledger(&report, &ledger),
        Err(ReviewError::DuplicateItemId(_))
    ));
}

#[test]
fn apply_rejects_empty_id_and_missing_fingerprint() {
    let report = sample_report();
    let mut ledger = review::init_ledger(&report);
    ledger.items[0].id.clear();

    assert_eq!(
        review::apply_ledger(&report, &ledger).unwrap_err(),
        ReviewError::MissingItemId
    );

    let mut ledger = review::init_ledger(&report);
    ledger.items[0].fingerprint.clear();

    assert!(matches!(
        review::apply_ledger(&report, &ledger),
        Err(ReviewError::MissingItemFingerprint(_))
    ));
}

#[test]
fn apply_rejects_kind_mismatch() {
    let report = sample_report();
    let mut ledger = review::init_ledger(&report);
    item_of_mut(&mut ledger, ReviewKind::Finding).kind = ReviewKind::Function;

    assert!(matches!(
        review::apply_ledger(&report, &ledger),
        Err(ReviewError::KindMismatch { .. })
    ));
}

#[test]
fn apply_rejects_decisions_missing_their_reasons() {
    let report = sample_report();
    let mut ledger = review::init_ledger(&report);
    ledger.items[0].status = ReviewStatus::Retained;

    assert!(matches!(
        review::apply_ledger(&report, &ledger),
        Err(ReviewError::Decision {
            problem: DecisionProblem::ReasonCategory,
            ..
        })
    ));

    ledger.items[0].reason_category = "semantics".to_owned();
    ledger.items[0].reason = "   ".to_owned();

    assert!(matches!(
        review::apply_ledger(&report, &ledger),
        Err(ReviewError::Decision {
            problem: DecisionProblem::Reason,
            ..
        })
    ));

    ledger.items[0].reason = "intentional".to_owned();

    assert!(matches!(
        review::apply_ledger(&report, &ledger),
        Err(ReviewError::Decision {
            problem: DecisionProblem::Evidence,
            ..
        })
    ));
}

#[test]
fn upstream_deferral_requires_an_upstream() {
    let report = sample_report();
    let mut ledger = review::init_ledger(&report);
    ledger.items[0].status = ReviewStatus::Deferred;
    ledger.items[0].reason_category = "upstream".to_owned();
    ledger.items[0].reason = "blocked".to_owned();
    ledger.items[0].evidence = "waiting for owner".to_owned();

    assert!(matches!(
        review::apply_ledger(&report, &ledger),
        Err(ReviewError::Decision {
            problem: DecisionProblem::Upstream,
            ..
        })
    ));

    ledger.items[0].upstream = "example/project#1".to_owned();

    assert!(review::apply_ledger(&report, &ledger).is_ok());
}

#[test]
fn apply_requires_matching_schema() {
    let report = sample_report();
    let mut ledger = review::init_ledger(&report);
    ledger.schema_version = "999".to_owned();

    assert!(matches!(
        review::apply_ledger(&report, &ledger),
        Err(ReviewError::LedgerSchema { .. })
    ));
}

#[test]
fn apply_requires_matching_report_identity() {
    let mut report = sample_report();
    let ledger = review::init_ledger(&report);
    report.config.language = "other".to_owned();

    assert_eq!(
        review::apply_ledger(&report, &ledger).unwrap_err(),
        ReviewError::ReportIdentityMismatch
    );

    let mut ledger = review::init_ledger(&sample_report());
    ledger.report_identity.clear();

    assert_eq!(
        review::apply_ledger(&sample_report(), &ledger).unwrap_err(),
        ReviewError::MissingReportIdentity
    );
}

#[test]
fn apply_rejects_an_invalid_report() {
    let mut report = sample_report();
    let ledger = review::init_ledger(&report);
    report.schema_version = "2".to_owned();

    assert_eq!(
        review::apply_ledger(&report, &ledger).unwrap_err(),
        ReviewError::Report(ValidationError::SchemaVersion("2".to_owned()))
    );
}

#[test]
fn apply_rejects_duplicate_native_ids() {
    let mut report = sample_report();
    report.findings.push(report.findings[0].clone());
    let ledger = review::init_ledger(&report);

    assert_eq!(
        review::apply_ledger(&report, &ledger).unwrap_err(),
        ReviewError::DuplicateInventoryId("finding-native".to_owned())
    );
}

#[test]
fn apply_rejects_native_items_without_fingerprints() {
    let mut report = sample_report();
    report.findings[0].fingerprint.clear();
    let ledger = review::init_ledger(&report);

    assert_eq!(
        review::apply_ledger(&report, &ledger).unwrap_err(),
        ReviewError::MissingInventoryFingerprint("finding-native".to_owned())
    );
}

#[test]
fn report_identity_ignores_line_moves_but_tracks_config() {
    let report = sample_report();
    let mut moved = sample_report();
    moved.findings[0].start.line += 5;
    let mut reconfigured = sample_report();
    reconfigured.config.high_complexity_threshold = 99;

    let identity = review::init_ledger(&report).report_identity;

    assert!(identity.starts_with("sha256:"));
    assert_eq!(identity, review::init_ledger(&moved).report_identity);
    assert_ne!(identity, review::init_ledger(&reconfigured).report_identity);
}

#[test]
fn current_ledger_builds_or_reapplies() {
    let mut report = sample_report();

    assert_eq!(
        review::current_ledger(&report).expect("fresh"),
        review::init_ledger(&report)
    );

    let mut ledger = review::init_ledger(&report);
    retain(
        item_of_mut(&mut ledger, ReviewKind::Finding),
        "reviewed source",
    );
    report.review = Some(ledger);

    let current = review::current_ledger(&report).expect("attached");

    assert_eq!(current.counts.retained, 1);

    report.review.as_mut().expect("ledger").report_identity = "sha256:other".to_owned();

    assert_eq!(
        review::current_ledger(&report).unwrap_err(),
        ReviewError::ReportIdentityMismatch
    );
}

#[test]
fn ledger_round_trips_through_json() {
    let ledger = review::init_ledger(&sample_report());
    let text = serde_json::to_string(&ledger).expect("encode");
    let decoded: ReviewLedger = serde_json::from_str(&text).expect("decode");

    assert_eq!(decoded, ledger);
}
