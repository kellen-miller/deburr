//! Where the audited source and this deburr build came from.

use std::path::Path;

use crate::report::{BuildProvenance, SourceProvenance};

#[derive(Debug, thiserror::Error)]
pub enum ProvenanceError {
    #[error("source provenance is not implemented yet")]
    NotImplemented,
}

/// Records the audited tree's git commit and dirty state.
///
/// # Errors
///
/// Returns an error when `git` cannot report the commit or status.
pub fn source(_dir: &Path, _manifest_digest: &str) -> Result<SourceProvenance, ProvenanceError> {
    Err(ProvenanceError::NotImplemented)
}

/// This deburr build's own identity.
#[must_use]
pub fn build() -> BuildProvenance {
    BuildProvenance {
        version: crate::VERSION.to_owned(),
    }
}
