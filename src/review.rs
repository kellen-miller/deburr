//! The review ledger: initialization and application on `Report` values.

use crate::report::{LEDGER_SCHEMA_VERSION, Report, ReviewCounts, ReviewLedger, ReviewedReport};

#[derive(Debug, thiserror::Error)]
pub enum ReviewError {
    #[error("review apply is not implemented yet")]
    NotImplemented,
    #[error("invalid review ledger: {0}")]
    Invalid(String),
}

/// Builds the initial ledger for `report`: every finding, every
/// high-complexity function, and every clone, all unreviewed.
///
/// Not implemented yet: returns an empty ledger.
#[must_use]
pub fn init_ledger(report: &Report) -> ReviewLedger {
    ReviewLedger {
        schema_version: LEDGER_SCHEMA_VERSION.to_owned(),
        report_identity: String::new(),
        source_manifest_digest: report.provenance.source.manifest_digest.clone(),
        items: Vec::new(),
        counts: ReviewCounts::default(),
    }
}

/// Validates `ledger` against `report` and attaches it.
///
/// # Errors
///
/// Returns an error when the ledger does not belong to the report or a
/// decision is incomplete.
pub fn apply_ledger(
    _report: &Report,
    _ledger: &ReviewLedger,
) -> Result<ReviewedReport, ReviewError> {
    Err(ReviewError::NotImplemented)
}
