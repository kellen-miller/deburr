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

/// Analyzes one parsed file. Function units and findings arrive with the
/// language milestones; the code-line map is real.
#[must_use]
pub fn analyze(_path: &str, _category: Category, syntax: &Syntax) -> FileAnalysis {
    FileAnalysis {
        functions: Vec::new(),
        findings: Vec::new(),
        code_lines: syntax.code_lines(),
    }
}
