//! Walks the audited tree and decides each file's category and status.

use std::path::Path;

use crate::audit::AuditOptions;
use crate::report::{Category, FileStatus};
use crate::syntax::Language;

/// One file found under the audited root.
#[derive(Debug)]
pub struct DiscoveredFile {
    /// Slash-separated path relative to the audited root.
    pub path: String,
    pub category: Category,
    pub status: FileStatus,
    pub reason: String,
    pub language: Option<Language>,
    /// Source bytes; empty for files that were not read.
    pub source: Vec<u8>,
}

/// The walk result: files in lexical order plus directory and symlink counts.
#[derive(Debug, Default)]
pub struct Discovery {
    pub files: Vec<DiscoveredFile>,
    pub symlinks: u32,
    pub excluded_directories: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum DiscoverError {
    #[error("discovery is not implemented yet")]
    NotImplemented,
}

/// Walks `root` in lexical order, never following symlinks.
///
/// # Errors
///
/// Returns an error when `root` cannot be read.
pub fn discover(_root: &Path, _options: &AuditOptions) -> Result<Discovery, DiscoverError> {
    Err(DiscoverError::NotImplemented)
}
