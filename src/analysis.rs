//! Turns a parsed file into function units, findings, and a per-line code map.

use crate::report::{Category, Finding, Function};
use crate::syntax::Syntax;

/// Everything measured in one file.
#[derive(Debug, Default)]
pub struct FileAnalysis {
    pub functions: Vec<Function>,
    pub findings: Vec<Finding>,
    /// Index `n - 1` is true when line `n` holds code.
    pub code_lines: Vec<bool>,
}

/// Analyzes one parsed file. Not implemented yet: returns an empty analysis.
#[must_use]
pub fn analyze(_path: &str, _category: Category, _syntax: &Syntax) -> FileAnalysis {
    FileAnalysis::default()
}
