//! Built-in clone detection: the only module that touches `cpd_core`.

use crate::analysis::FileAnalysis;
use crate::report::{Category, CodeClone, DuplicationCategory};
use crate::syntax::Syntax;

/// Detection settings exposed as `--min-clone-tokens` and `--min-clone-lines`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DuplicationSettings {
    pub min_tokens: usize,
    pub min_lines: usize,
}

/// One analyzed file, borrowed for detection.
#[derive(Debug, Clone, Copy)]
pub struct AnalyzedFile<'a> {
    pub path: &'a str,
    pub syntax: &'a Syntax,
    pub analysis: &'a FileAnalysis,
}

/// The measurement and clones for one category.
#[derive(Debug)]
pub struct CategoryDuplication {
    pub measurement: DuplicationCategory,
    pub clones: Vec<CodeClone>,
}

#[derive(Debug, thiserror::Error)]
pub enum DuplicationError {
    #[error("duplication detection is not implemented yet")]
    NotImplemented,
}

/// Finds clones among the files of one category.
///
/// # Errors
///
/// Returns an error when detection cannot run.
pub fn detect(
    _files: &[AnalyzedFile<'_>],
    _category: Category,
    _settings: &DuplicationSettings,
) -> Result<CategoryDuplication, DuplicationError> {
    Err(DuplicationError::NotImplemented)
}
