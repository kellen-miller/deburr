//! Parsing: one tree-sitter parse per file feeds line counts, functions,
//! findings, clone identity, and duplication tokens.

use std::ops::Range;

use tree_sitter::{Node, Parser, Point};

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

    fn grammar(self) -> tree_sitter::Language {
        match self {
            Self::Go => tree_sitter_go::LANGUAGE.into(),
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        }
    }

    /// Nodes whose named children are statements that get a synthetic
    /// terminator token.
    fn is_statement_container(self, kind: &str) -> bool {
        match self {
            Self::Go => kind == "statement_list",
            Self::TypeScript | Self::Tsx => matches!(kind, "program" | "statement_block"),
        }
    }

    /// Nodes that tree-sitter splits into children but that read as one token.
    fn is_atomic(self, kind: &str) -> bool {
        match self {
            Self::Go => matches!(
                kind,
                "interpreted_string_literal" | "raw_string_literal" | "rune_literal"
            ),
            Self::TypeScript | Self::Tsx => matches!(kind, "string" | "template_string" | "regex"),
        }
    }

    fn is_comment(self, kind: &str) -> bool {
        match self {
            Self::Go => kind == "comment",
            Self::TypeScript | Self::Tsx => matches!(kind, "comment" | "html_comment"),
        }
    }
}

/// A code token: a non-comment leaf (strings are single tokens) or a
/// synthetic statement terminator. A terminator has an empty byte range;
/// use [`Syntax::token_text`] to get the text of either kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub bytes: Range<usize>,
    pub start: Position,
    /// Inclusive end.
    pub end: Position,
}

impl Token {
    /// Whether this is a synthetic statement terminator (`;`).
    #[must_use]
    pub fn is_terminator(&self) -> bool {
        self.bytes.is_empty()
    }
}

/// A parsed file.
#[derive(Debug)]
pub struct Syntax {
    pub language: Language,
    pub source: Vec<u8>,
    pub tree: tree_sitter::Tree,
    pub tokens: Vec<Token>,
}

impl Syntax {
    /// The text of `token`: its source bytes, or `;` for a terminator.
    #[must_use]
    pub fn token_text(&self, token: &Token) -> &[u8] {
        if token.is_terminator() {
            b";"
        } else {
            &self.source[token.bytes.clone()]
        }
    }

    /// Index `n - 1` is true when line `n` holds a code token. Comments,
    /// blank lines, and synthetic terminators never mark a line.
    #[must_use]
    pub fn code_lines(&self) -> Vec<bool> {
        let mut code = vec![false; line_count(&self.source)];

        for token in self.tokens.iter().filter(|token| !token.is_terminator()) {
            let first = token.start.line.saturating_sub(1) as usize;
            let last = (token.end.line as usize).min(code.len());

            for line in code.iter_mut().take(last).skip(first) {
                *line = true;
            }
        }

        code
    }
}

/// Newline-terminated lines, plus one when the last byte is not a newline.
#[must_use]
pub fn line_count(source: &[u8]) -> usize {
    let newlines = source.iter().filter(|&&byte| byte == b'\n').count();

    match source.last() {
        Some(&last) if last != b'\n' => newlines + 1,
        _ => newlines,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SyntaxError {
    /// tree-sitter could not load the grammar or produced no tree. The
    /// source is handed back so callers can still digest it.
    #[error("tree-sitter failed: {reason}")]
    Unavailable { reason: String, bytes: Vec<u8> },
    /// The tree has error or missing nodes, or the file fails the language's
    /// structural validity check. `recovered` is the tree and tokens after
    /// error recovery, so callers can still count lines.
    #[error("syntax error at {line}:{column}")]
    Invalid {
        line: u32,
        column: u32,
        recovered: Box<Syntax>,
    },
}

impl SyntaxError {
    /// What survived the failure: the recovered syntax, or just the source
    /// when tree-sitter produced nothing.
    ///
    /// # Errors
    ///
    /// Returns the source bytes when no tree was produced.
    pub fn recovered(self) -> Result<Syntax, Vec<u8>> {
        match self {
            Self::Invalid { recovered, .. } => Ok(*recovered),
            Self::Unavailable { bytes, .. } => Err(bytes),
        }
    }
}

/// Parses `source` as `language`.
///
/// # Errors
///
/// Returns [`SyntaxError::Invalid`] when the tree contains error or missing
/// nodes, or fails the language's structural validity check (Go: the first
/// non-comment top-level node must be the package clause and every top-level
/// node must be a declaration). Returns [`SyntaxError::Unavailable`] when
/// tree-sitter itself fails.
pub fn parse(language: Language, source: Vec<u8>) -> Result<Syntax, SyntaxError> {
    let mut parser = Parser::new();

    if let Err(error) = parser.set_language(&language.grammar()) {
        return Err(SyntaxError::Unavailable {
            reason: error.to_string(),
            bytes: source,
        });
    }

    let Some(tree) = parser.parse(&source, None) else {
        return Err(SyntaxError::Unavailable {
            reason: "parser produced no tree".to_owned(),
            bytes: source,
        });
    };

    let root = tree.root_node();

    let mut problem = first_error(root);

    if problem.is_none() && language == Language::Go {
        problem = go_structure_problem(root);
    }

    let tokens = collect_tokens(language, root, &source);
    let syntax = Syntax {
        language,
        source,
        tree,
        tokens,
    };

    match problem {
        None => Ok(syntax),
        Some(point) => Err(SyntaxError::Invalid {
            line: u32::try_from(point.row + 1).unwrap_or(u32::MAX),
            column: u32::try_from(point.column + 1).unwrap_or(u32::MAX),
            recovered: Box::new(syntax),
        }),
    }
}

/// The start of the first `ERROR` or `MISSING` node in pre-order.
fn first_error(root: Node<'_>) -> Option<Point> {
    if !root.has_error() {
        return None;
    }

    let mut cursor = root.walk();

    loop {
        let node = cursor.node();

        if node.is_error() || node.is_missing() {
            return Some(node.start_position());
        }

        if node.has_error() && cursor.goto_first_child() {
            continue;
        }

        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return None;
            }
        }
    }
}

/// tree-sitter-go accepts top-level statements and files without a package
/// clause; `go/parser` rejects both, so deburr does too.
fn go_structure_problem(root: Node<'_>) -> Option<Point> {
    let mut cursor = root.walk();
    let mut seen_first = false;

    for child in root.named_children(&mut cursor) {
        if child.kind() == "comment" {
            continue;
        }

        let allowed = if seen_first {
            matches!(
                child.kind(),
                "package_clause"
                    | "import_declaration"
                    | "function_declaration"
                    | "method_declaration"
                    | "type_declaration"
                    | "var_declaration"
                    | "const_declaration"
            )
        } else {
            child.kind() == "package_clause"
        };

        if !allowed {
            return Some(child.start_position());
        }

        seen_first = true;
    }

    if seen_first {
        None
    } else {
        Some(Point { row: 0, column: 0 })
    }
}

/// Collects code tokens in source order: non-comment leaves, string-like
/// nodes as single tokens, and a synthetic `;` after each named child of a
/// statement container. An explicit `;` that ends a statement is replaced by
/// the terminator, so `a();` and `a()` plus a newline tokenize alike.
fn collect_tokens(language: Language, root: Node<'_>, source: &[u8]) -> Vec<Token> {
    let mut tokens: Vec<Token> = Vec::new();
    let mut cursor = root.walk();
    let mut exiting = false;

    loop {
        let node = cursor.node();

        if !exiting {
            let kind = node.kind();
            let in_container = node
                .parent()
                .is_some_and(|parent| language.is_statement_container(parent.kind()));

            if language.is_comment(kind) {
                exiting = true;
            } else if node.child_count() == 0 || language.is_atomic(kind) {
                // A container's own `;` between statements is covered by the
                // terminator emitted after the preceding statement.
                let covered_by_terminator = in_container && !node.is_named() && kind == ";";

                if !covered_by_terminator && node.end_byte() > node.start_byte() {
                    tokens.push(Token {
                        bytes: node.byte_range(),
                        start: position(node.start_position()),
                        end: inclusive_end(source, node),
                    });
                }

                exiting = true;
            } else if !cursor.goto_first_child() {
                exiting = true;
            }

            continue;
        }

        let terminated = node.is_named()
            && !language.is_comment(node.kind())
            && node.end_byte() > node.start_byte()
            && node
                .parent()
                .is_some_and(|parent| language.is_statement_container(parent.kind()));

        if terminated {
            if tokens.last().is_some_and(|last| {
                !last.is_terminator()
                    && last.bytes.end == node.end_byte()
                    && &source[last.bytes.clone()] == b";"
            }) {
                tokens.pop();
            }

            let end = inclusive_end(source, node);
            tokens.push(Token {
                bytes: node.end_byte()..node.end_byte(),
                start: end,
                end,
            });
        }

        if cursor.goto_next_sibling() {
            exiting = false;
        } else if !cursor.goto_parent() {
            return tokens;
        }
    }
}

fn position(point: Point) -> Position {
    Position {
        line: u32::try_from(point.row + 1).unwrap_or(u32::MAX),
        column: u32::try_from(point.column + 1).unwrap_or(u32::MAX),
    }
}

/// The position of the last byte of `node`. tree-sitter's end point is
/// exclusive, which is the inclusive 1-based column unless the node ends with
/// a newline; then the end is the previous line's last column.
fn inclusive_end(source: &[u8], node: Node<'_>) -> Position {
    let end = node.end_position();

    if end.column > 0 || end.row == 0 {
        return Position {
            line: u32::try_from(end.row + 1).unwrap_or(u32::MAX),
            column: u32::try_from(end.column.max(1)).unwrap_or(u32::MAX),
        };
    }

    let newline = node.end_byte() - 1;
    let line_start = source[..newline]
        .iter()
        .rposition(|&byte| byte == b'\n')
        .map_or(0, |index| index + 1);

    Position {
        line: u32::try_from(end.row).unwrap_or(u32::MAX),
        column: u32::try_from((newline - line_start).max(1)).unwrap_or(u32::MAX),
    }
}
