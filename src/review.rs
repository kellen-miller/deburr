//! The review ledger: initialization and application on `Report` values.
//!
//! A ledger records, per finding, high-complexity function, and clone,
//! whether it was refactored, retained, or deferred, and why. Applying a
//! ledger to a later report keeps decisions whose fingerprint still matches
//! and resets the rest to `unreviewed` with `stale = true`.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::identity::sha256_prefixed;
use crate::report::{
    AnalyzerIdentity, ConfigSummary, LEDGER_SCHEMA_VERSION, Location, Report, ReviewCounts,
    ReviewDecision, ReviewItem, ReviewKind, ReviewLedger, ReviewStatus, ReviewedReport, Scope,
    ValidationError,
};

/// Deferral reason category that must name an upstream owner.
const UPSTREAM_REASON_CATEGORY: &str = "upstream";

/// What is missing from a decided item.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DecisionProblem {
    #[error("reason_category is required for a decided item")]
    ReasonCategory,
    #[error("reason is required for a decided item")]
    Reason,
    #[error("evidence is required for a decided item")]
    Evidence,
    #[error("upstream is required for an upstream deferral")]
    Upstream,
}

/// Why a ledger cannot be applied to a report.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ReviewError {
    #[error(transparent)]
    Report(#[from] ValidationError),
    #[error("review schema_version {ledger:?} does not match ledger schema {expected:?}")]
    LedgerSchema {
        ledger: String,
        expected: &'static str,
    },
    #[error("review report_identity is required")]
    MissingReportIdentity,
    #[error("review report_identity does not match report")]
    ReportIdentityMismatch,
    #[error("review inventory item {0} has no id")]
    MissingInventoryId(usize),
    #[error("review inventory item {0:?} has no fingerprint")]
    MissingInventoryFingerprint(String),
    #[error("review inventory contains duplicate id {0:?}")]
    DuplicateInventoryId(String),
    #[error("review item id is required")]
    MissingItemId,
    #[error("review ledger contains duplicate id {0:?}")]
    DuplicateItemId(String),
    #[error("review ledger contains unknown id {0:?}")]
    UnknownItemId(String),
    #[error("review item {id:?} kind {ledger} does not match current kind {current}")]
    KindMismatch {
        id: String,
        ledger: &'static str,
        current: &'static str,
    },
    #[error("review item {id:?}: {problem}")]
    Decision {
        id: String,
        problem: DecisionProblem,
    },
    #[error("review item {0:?} fingerprint is required")]
    MissingItemFingerprint(String),
}

impl ReviewKind {
    /// The serialized name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Clone => "clone",
            Self::Finding => "finding",
            Self::Function => "function",
        }
    }
}

/// The fields that identify which report a ledger belongs to.
#[derive(Serialize)]
struct ReportIdentity<'a> {
    analyzer: &'a AnalyzerIdentity,
    schema_version: &'a str,
    scope: &'a Scope,
    config: &'a ConfigSummary,
}

/// Builds the initial ledger for `report`: every finding, every
/// high-complexity function, and every clone, all unreviewed. Any ledger
/// already attached to `report` is ignored.
#[must_use]
pub fn init_ledger(report: &Report) -> ReviewLedger {
    assemble(report, report_identity(report), inventory(report))
}

/// Validates `ledger` against `report` and attaches the result. Items
/// missing from `ledger` stay unreviewed; items whose fingerprint changed
/// stay in the inventory with `stale = true` and their prior decision kept
/// in `previous`. Any ledger already attached to `report` is ignored.
///
/// # Errors
///
/// Returns an error when the ledger does not belong to the report (schema or
/// report identity mismatch), names an unknown, duplicate, or wrong-kind
/// item, or records an incomplete decision. Also returned when the report's
/// own inventory has missing or duplicate IDs or a missing fingerprint.
pub fn apply_ledger(report: &Report, ledger: &ReviewLedger) -> Result<ReviewedReport, ReviewError> {
    let applied = apply(report, ledger)?;
    let mut reviewed = report.clone();
    reviewed.review = Some(applied);

    Ok(ReviewedReport(reviewed))
}

/// The ledger `report` carries, re-applied to its own inventory so counts
/// and staleness are current; a fresh ledger when it carries none.
///
/// # Errors
///
/// Returns an error when the attached ledger fails [`apply_ledger`]'s checks.
pub fn current_ledger(report: &Report) -> Result<ReviewLedger, ReviewError> {
    match &report.review {
        Some(attached) => apply(report, attached),
        None => Ok(init_ledger(report)),
    }
}

fn apply(report: &Report, ledger: &ReviewLedger) -> Result<ReviewLedger, ReviewError> {
    report.validate()?;

    if ledger.schema_version != LEDGER_SCHEMA_VERSION {
        return Err(ReviewError::LedgerSchema {
            ledger: ledger.schema_version.clone(),
            expected: LEDGER_SCHEMA_VERSION,
        });
    }

    let identity = report_identity(report);

    if ledger.report_identity.is_empty() {
        return Err(ReviewError::MissingReportIdentity);
    }

    if ledger.report_identity != identity {
        return Err(ReviewError::ReportIdentityMismatch);
    }

    let mut current = inventory(report);
    let mut index_by_id = HashMap::with_capacity(current.len());

    for (index, item) in current.iter().enumerate() {
        if item.id.is_empty() {
            return Err(ReviewError::MissingInventoryId(index));
        }

        if item.fingerprint.is_empty() {
            return Err(ReviewError::MissingInventoryFingerprint(item.id.clone()));
        }

        if index_by_id.insert(item.id.clone(), index).is_some() {
            return Err(ReviewError::DuplicateInventoryId(item.id.clone()));
        }
    }

    let mut decisions: Vec<&ReviewItem> = ledger.items.iter().collect();
    decisions.sort_by(|left, right| left.id.cmp(&right.id));

    let mut seen = HashSet::with_capacity(decisions.len());
    let report_manifest = &report.provenance.source.manifest_digest;

    for decision in decisions {
        let index = bind_decision(decision, &current, &index_by_id, &mut seen)?;
        let item = &mut current[index];

        if decision.fingerprint == item.fingerprint
            && !decision.stale
            && decision_allowed(
                item,
                decision,
                &ledger.source_manifest_digest,
                report_manifest,
            )
        {
            item.status = decision.status;
            item.reason_category.clone_from(&decision.reason_category);
            item.reason.clone_from(&decision.reason);
            item.evidence.clone_from(&decision.evidence);
            item.upstream.clone_from(&decision.upstream);
            item.previous = None;
        } else {
            item.stale = true;
            item.status = ReviewStatus::Unreviewed;
            item.previous = previous_decision(decision);
        }
    }

    Ok(assemble(report, identity, current))
}

/// Checks that `decision` names exactly one current inventory item of the
/// same kind and is complete, and returns that item's index.
fn bind_decision<'a>(
    decision: &'a ReviewItem,
    current: &[ReviewItem],
    index_by_id: &HashMap<String, usize>,
    seen: &mut HashSet<&'a str>,
) -> Result<usize, ReviewError> {
    if decision.id.is_empty() {
        return Err(ReviewError::MissingItemId);
    }

    if !seen.insert(decision.id.as_str()) {
        return Err(ReviewError::DuplicateItemId(decision.id.clone()));
    }

    let Some(&index) = index_by_id.get(&decision.id) else {
        return Err(ReviewError::UnknownItemId(decision.id.clone()));
    };

    if decision.kind != current[index].kind {
        return Err(ReviewError::KindMismatch {
            id: decision.id.clone(),
            ledger: decision.kind.as_str(),
            current: current[index].kind.as_str(),
        });
    }

    if let Some(problem) = incomplete_decision(decision) {
        return Err(ReviewError::Decision {
            id: decision.id.clone(),
            problem,
        });
    }

    if decision.fingerprint.is_empty() {
        return Err(ReviewError::MissingItemFingerprint(decision.id.clone()));
    }

    Ok(index)
}

/// A decision on an identity-ambiguous item only holds for the exact source
/// snapshot it was made against; an unreviewed one always holds.
fn decision_allowed(
    item: &ReviewItem,
    decision: &ReviewItem,
    ledger_manifest: &str,
    report_manifest: &str,
) -> bool {
    if !item.identity_ambiguous && !decision.identity_ambiguous {
        return true;
    }

    if decision.status == ReviewStatus::Unreviewed {
        return true;
    }

    !ledger_manifest.is_empty() && ledger_manifest == report_manifest
}

fn incomplete_decision(item: &ReviewItem) -> Option<DecisionProblem> {
    if item.status == ReviewStatus::Unreviewed {
        return None;
    }

    if item.reason_category.trim().is_empty() {
        return Some(DecisionProblem::ReasonCategory);
    }

    if item.reason.trim().is_empty() {
        return Some(DecisionProblem::Reason);
    }

    if item.evidence.trim().is_empty() {
        return Some(DecisionProblem::Evidence);
    }

    if item.status == ReviewStatus::Deferred
        && item.reason_category == UPSTREAM_REASON_CATEGORY
        && item.upstream.trim().is_empty()
    {
        return Some(DecisionProblem::Upstream);
    }

    None
}

/// The decision a stale item keeps: its own earlier `previous` if it has
/// one, else whatever decision `item` records, else nothing.
fn previous_decision(item: &ReviewItem) -> Option<ReviewDecision> {
    if item.previous.is_some() {
        return item.previous.clone();
    }

    let undecided = item.status == ReviewStatus::Unreviewed
        && item.reason_category.trim().is_empty()
        && item.reason.trim().is_empty()
        && item.evidence.trim().is_empty()
        && item.upstream.trim().is_empty();

    if undecided {
        return None;
    }

    Some(ReviewDecision {
        fingerprint: item.fingerprint.clone(),
        status: item.status,
        reason_category: item.reason_category.clone(),
        reason: item.reason.clone(),
        evidence: item.evidence.clone(),
        upstream: item.upstream.clone(),
    })
}

/// Every finding, every high-complexity function, and every clone as an
/// unreviewed item, ordered by kind (clone, finding, function) then ID.
fn inventory(report: &Report) -> Vec<ReviewItem> {
    let mut items = Vec::with_capacity(
        report.findings.len() + report.functions.len() + report.duplication.clones.len(),
    );

    for finding in &report.findings {
        items.push(unreviewed(
            &finding.id,
            ReviewKind::Finding,
            &finding.fingerprint,
            finding.identity_ambiguous,
            vec![Location {
                path: finding.path.clone(),
                start: finding.start,
                end: finding.end,
            }],
        ));
    }

    for function in report.functions.iter().filter(|f| f.high_complexity) {
        items.push(unreviewed(
            &function.id,
            ReviewKind::Function,
            &function.fingerprint,
            function.identity_ambiguous,
            vec![Location {
                path: function.path.clone(),
                start: function.start,
                end: function.end,
            }],
        ));
    }

    for clone in &report.duplication.clones {
        items.push(unreviewed(
            &clone.id,
            ReviewKind::Clone,
            &clone.fingerprint,
            clone.identity_ambiguous,
            clone
                .endpoints
                .iter()
                .map(|endpoint| Location {
                    path: endpoint.path.clone(),
                    start: endpoint.start,
                    end: endpoint.end,
                })
                .collect(),
        ));
    }

    items.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.id.cmp(&right.id))
    });

    items
}

fn unreviewed(
    id: &str,
    kind: ReviewKind,
    fingerprint: &str,
    identity_ambiguous: bool,
    locations: Vec<Location>,
) -> ReviewItem {
    ReviewItem {
        previous: None,
        id: id.to_owned(),
        kind,
        fingerprint: fingerprint.to_owned(),
        status: ReviewStatus::Unreviewed,
        reason_category: String::new(),
        reason: String::new(),
        evidence: String::new(),
        upstream: String::new(),
        locations,
        stale: false,
        identity_ambiguous,
    }
}

fn assemble(report: &Report, report_identity: String, items: Vec<ReviewItem>) -> ReviewLedger {
    let mut counts = ReviewCounts {
        total: u32::try_from(items.len()).unwrap_or(u32::MAX),
        ..ReviewCounts::default()
    };

    for item in &items {
        match item.status {
            ReviewStatus::Unreviewed => counts.unreviewed += 1,
            ReviewStatus::Refactored => counts.refactored += 1,
            ReviewStatus::Retained => counts.retained += 1,
            ReviewStatus::Deferred => counts.deferred += 1,
        }

        if item.stale {
            counts.stale += 1;
        }
    }

    ReviewLedger {
        schema_version: LEDGER_SCHEMA_VERSION.to_owned(),
        report_identity,
        source_manifest_digest: report.provenance.source.manifest_digest.clone(),
        items,
        counts,
    }
}

/// `sha256:<hex>` over the compact JSON of analyzer, schema version, scope,
/// and config; empty if encoding fails (it cannot for these types).
fn report_identity(report: &Report) -> String {
    serde_json::to_vec(&ReportIdentity {
        analyzer: &report.analyzer,
        schema_version: &report.schema_version,
        scope: &report.scope,
        config: &report.config,
    })
    .map(|bytes| sha256_prefixed(&bytes))
    .unwrap_or_default()
}
