//! Parsing: one tree-sitter parse per file feeds line counts, functions,
//! findings, clone identity, and duplication tokens.

use std::ops::Range;

use crate::report::Position;

pub mod go;
pub mod typescript;

/// Source languages deburr can parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    Go,
    TypeScript,
    Tsx,
}

/// Per-language node-kind tables and naming. Filled in by the language milestones.
#[derive(Debug)]
pub struct LanguageRules;

impl Language {
    /// The rules table for this language.
    #[must_use]
    pub fn rules(self) -> &'static LanguageRules {
        match self {
            Self::Go => &go::RULES,
            Self::TypeScript | Self::Tsx => &typescript::RULES,
        }
    }
}

/// A code token: a non-comment leaf (strings are single tokens) or a
/// synthetic statement terminator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub bytes: Range<usize>,
    pub start: Position,
    /// Inclusive end.
    pub end: Position,
}

/// A parsed file.
#[derive(Debug)]
pub struct Syntax {
    pub language: Language,
    pub source: Vec<u8>,
    pub tree: tree_sitter::Tree,
    pub tokens: Vec<Token>,
}

#[derive(Debug, thiserror::Error)]
pub enum SyntaxError {
    #[error("parsing is not implemented yet")]
    NotImplemented,
    #[error("syntax error at {line}:{column}")]
    Invalid { line: u32, column: u32 },
}

/// Parses `source` as `language`.
///
/// # Errors
///
/// Returns [`SyntaxError::Invalid`] when the tree contains error or missing
/// nodes, or fails the language's structural validity check.
pub fn parse(_language: Language, _source: Vec<u8>) -> Result<Syntax, SyntaxError> {
    Err(SyntaxError::NotImplemented)
}
