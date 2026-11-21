//! Compares two reports from the same analyzer contract.

use crate::report::{Comparison, DuplicationComparison, Report, ReviewLedger, SCHEMA_VERSION};
use crate::review;

fn ledger_of(report: &Report) -> ReviewLedger {
    report
        .review
        .clone()
        .unwrap_or_else(|| review::init_ledger(report))
}

/// Compares `before` and `after`, both already validated.
///
/// Not implemented yet: the result is marked incompatible and incomplete
/// with a reason saying so, so the CLI exits 1 without drawing conclusions.
#[must_use]
pub fn compare(before: &Report, after: &Report) -> Comparison {
    Comparison {
        metrics_delta: None,
        after_provenance: after.provenance.clone(),
        before_provenance: before.provenance.clone(),
        before_analyzer: before.analyzer.clone(),
        after_analyzer: after.analyzer.clone(),
        schema_version: SCHEMA_VERSION.to_owned(),
        reason: "compare is not implemented yet".to_owned(),
        ambiguous_clones: Vec::new(),
        function_changes: Vec::new(),
        resolved_findings: Vec::new(),
        ambiguous_functions: Vec::new(),
        clone_changes: Vec::new(),
        ambiguous_findings: Vec::new(),
        new_findings: Vec::new(),
        finding_changes: Vec::new(),
        before_scope: before.scope.clone(),
        after_scope: after.scope.clone(),
        duplication: DuplicationComparison {
            reason: String::new(),
            before_status: before.duplication.status,
            after_status: after.duplication.status,
            before_clones: Vec::new(),
            after_clones: Vec::new(),
            before_config: before.config.duplication.clone(),
            after_config: after.config.duplication.clone(),
            comparable: false,
        },
        before_metrics: before.metrics.clone(),
        after_metrics: after.metrics.clone(),
        before_config: before.config.clone(),
        after_config: after.config.clone(),
        before_review: ledger_of(before),
        after_review: ledger_of(after),
        before_coverage: before.coverage.clone(),
        after_coverage: after.coverage.clone(),
        complete: false,
        compatible: false,
    }
}
