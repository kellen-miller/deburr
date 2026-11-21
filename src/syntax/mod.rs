//! Parsing: one tree-sitter parse per file feeds line counts, functions,
//! findings, clone identity, and duplication tokens.
//!
//! Everything language-specific is one [`LanguageRules`] table per language
//! file; the analysis code never matches on a language.

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

/// Everything the engine needs to know about one language: node-kind tables
/// plus a few shape functions for what a table cannot say.
#[derive(Debug)]
pub struct LanguageRules {
    pub tokens: TokenRules,
    /// Node kinds that are function units when they have a `body`.
    pub function_kinds: &'static [&'static str],
    /// Node kinds that each add one decision point.
    pub decision_kinds: &'static [&'static str],
    /// Node kinds that deepen nesting inside their subtree.
    pub nesting_kinds: &'static [&'static str],
    /// The node kind of a binary expression, whose `operator` field is
    /// checked against `short_circuit_operators`.
    pub binary_kind: &'static str,
    pub short_circuit_operators: &'static [&'static str],
    /// How function units are named and identified.
    pub naming: UnitNaming,
    /// The package (or module) segment of function IDs.
    pub package_name: fn(Node<'_>, &[u8]) -> String,
    /// Statement shapes shared by signals and findings.
    pub statements: StatementRules,
    /// Structural signals beyond nesting; `None` when the language has none.
    pub signals: Option<SignalRules>,
    pub findings: FindingRules,
}

/// How a parse tree becomes the code tokens that every fingerprint, equality
/// check, and line count is computed from.
#[derive(Debug)]
pub struct TokenRules {
    /// Nodes whose named children are statements that each get a synthetic
    /// terminator token.
    pub statement_containers: &'static [&'static str],
    /// Nodes that tree-sitter splits into children but that read as one token.
    pub atomic: &'static [&'static str],
    /// Children that hold code. An atomic node with one of these as a direct
    /// child is tokenized by its leaves instead: a function written inside a
    /// template literal's `${..}` is still a function, and must have tokens
    /// of its own to be measured and compared.
    pub code_children: &'static [&'static str],
    pub comments: &'static [&'static str],
}

/// A function unit's enclosing unit, for languages that name by nesting.
#[derive(Debug, Clone, Copy)]
pub struct Enclosing<'name, 'tree> {
    pub node: Node<'tree>,
    pub name: &'name str,
}

/// How a language names function units, which also decides how their
/// identities are derived.
#[derive(Debug)]
pub enum UnitNaming {
    /// Go. Declarations carry their own names; a literal is `file.funcN` or
    /// `Parent.funcN`. A nested unit is identified by what it contains
    /// (`Parent.anon-<digest>`), so it keeps its identity when siblings move.
    Positional {
        /// The declared name of a function node, if it has one.
        declared_name: fn(Node<'_>, &[u8]) -> Option<String>,
    },
    /// TypeScript. A name is built from the enclosing classes and function
    /// (`Class.method`, `outer.inner`); a unit with no name at all is
    /// `anon-<digest>`. The identity is the name, with `#2`, `#3` for repeats.
    Lexical {
        /// The composed name of a function node, empty when it has none.
        name: fn(Node<'_>, &[u8], Option<Enclosing<'_, '_>>) -> String,
    },
}

/// Where statements live in the tree.
#[derive(Debug)]
pub struct StatementRules {
    pub comment_kind: &'static str,
    /// A braced body.
    pub block_kind: &'static str,
    /// The child of a block that holds its statements, when it has one (Go).
    /// Otherwise the statements are the block's own named children.
    pub statement_list_kind: Option<&'static str>,
    pub if_kind: &'static str,
    pub return_kind: &'static str,
}

/// What a call expression is called, for assertion matching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallTarget {
    /// The called function's name.
    pub name: String,
    /// The identifier a selector call is made on (`assert` in
    /// `assert.Equal(..)`), when the callee is a plain selector.
    pub receiver: Option<String>,
}

/// The shapes behind `flat_guards`, `field_mappings`, and
/// `assertion_like_calls`.
#[derive(Debug)]
pub struct SignalRules {
    /// Lone statements that make an `if` a guard clause.
    pub guard_kinds: &'static [&'static str],
    pub composite_kind: &'static str,
    /// Composite literal types whose keyed elements count as field mappings.
    pub composite_type_kinds: &'static [&'static str],
    pub keyed_element_kind: &'static str,
    /// The node that wraps a key or value inside a keyed element.
    pub element_kind: &'static str,
    pub mapping_key_kinds: &'static [&'static str],
    pub mapping_value_kinds: &'static [&'static str],
    pub call_kinds: &'static [&'static str],
    /// Names and receivers of a call, when `node` is one of `call_kinds`.
    pub call_target: fn(Node<'_>, &[u8]) -> Option<CallTarget>,
    /// Lowercase name prefixes that mark an assertion.
    pub assertion_prefixes: &'static [&'static str],
    /// Lowercase receivers whose every call is an assertion.
    pub assertion_receivers: &'static [&'static str],
    /// Lowercase receivers that are test handles...
    pub testing_receivers: &'static [&'static str],
    /// ...and the lowercase methods on them that assert.
    pub testing_methods: &'static [&'static str],
}

/// The shapes behind the two findings.
#[derive(Debug)]
pub struct FindingRules {
    pub shape: BranchShape,
    pub true_kind: &'static str,
    pub false_kind: &'static str,
    pub comparison_operators: &'static [&'static str],
    pub redundant_boolean_message: &'static str,
    pub duplicate_branches_message: &'static str,
}

/// Where a language's two findings look for an `if` and its other branch.
#[derive(Debug)]
pub enum BranchShape {
    /// Go. A redundant boolean return is an `if` and the `return` after it
    /// in one block, in a function declared to return a single `bool`. Branch
    /// duplication compares the two blocks of an `if`/`else`.
    Sequenced {
        /// Function kinds that may own a redundant-boolean-return finding.
        redundant_owner_kinds: &'static [&'static str],
        /// Whether a function declares exactly one unnamed `bool` result.
        returns_plain_bool: fn(Node<'_>, &[u8]) -> bool,
    },
    /// TypeScript. The other branch is the `else` or, with none, the next
    /// statement in the same block. Either finding may fire in any function,
    /// and at most one fires per `if`.
    Conditional {
        else_kind: &'static str,
        /// The node around an `if` condition.
        parenthesized_kind: &'static str,
    },
}

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
    /// Explicit `;` leaves that a synthetic terminator replaced in `tokens`.
    /// They are not token text, but like every non-comment leaf they mark the
    /// lines they sit on, so a `;` alone on a line is a code line.
    pub semicolons: Vec<Token>,
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

    /// Index `n - 1` is true when line `n` holds a code token or an explicit
    /// `;`. Comments, blank lines, and synthetic terminators never mark a
    /// line.
    #[must_use]
    pub fn code_lines(&self) -> Vec<bool> {
        let mut code = vec![false; line_count(&self.source)];
        let real = self.tokens.iter().filter(|token| !token.is_terminator());

        for token in real.chain(&self.semicolons) {
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

    let (tokens, semicolons) = collect_tokens(&language.rules().tokens, root, &source);
    let syntax = Syntax {
        language,
        source,
        tree,
        tokens,
        semicolons,
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

/// The start of the first `ERROR` or `MISSING` node inside the tree, in
/// pre-order. When recovery gives up, tree-sitter makes the root itself an
/// `ERROR` spanning the whole file, which says nothing about where the
/// problem is, so the root is never reported while a descendant is. A root
/// `ERROR` with no flagged descendant is reported at the root.
fn first_error(root: Node<'_>) -> Option<Point> {
    if !root.has_error() {
        return None;
    }

    let mut cursor = root.walk();

    if cursor.goto_first_child() {
        loop {
            let node = cursor.node();

            if node.is_error() || node.is_missing() {
                return Some(node.start_position());
            }

            if node.has_error() && cursor.goto_first_child() {
                continue;
            }

            while !cursor.goto_next_sibling() {
                if !cursor.goto_parent() || cursor.node() == root {
                    return Some(stray_token_position(root));
                }
            }
        }
    }

    Some(stray_token_position(root))
}

/// When recovery wraps the whole file in an `ERROR` root with no error node
/// inside it, the well-formed items survive as named children and the text
/// that broke the parse is left as unnamed tokens; the first of those is the
/// position worth reporting. Only an error with no such token falls back to
/// the root itself.
fn stray_token_position(root: Node<'_>) -> Point {
    let mut cursor = root.walk();
    let stray = root.children(&mut cursor).find(|child| !child.is_named());

    stray.map_or_else(|| root.start_position(), |child| child.start_position())
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
/// the terminator, so `a();` and `a()` plus a newline tokenize alike. The
/// replaced `;` leaves are returned separately because they still mark lines.
fn collect_tokens(rules: &TokenRules, root: Node<'_>, source: &[u8]) -> (Vec<Token>, Vec<Token>) {
    let mut tokens: Vec<Token> = Vec::new();
    let mut semicolons: Vec<Token> = Vec::new();
    let mut cursor = root.walk();
    // Kinds of the current node's ancestors, innermost last. `Node::parent`
    // re-walks from the root, which is quadratic on wide files such as
    // generated protobuf code, so the walk keeps its own ancestry.
    let mut ancestors: Vec<&str> = Vec::new();
    let mut exiting = false;

    loop {
        let node = cursor.node();
        let in_container = ancestors
            .last()
            .is_some_and(|parent| rules.statement_containers.contains(parent));

        if !exiting {
            let kind = node.kind();

            let atomic = rules.atomic.contains(&kind)
                && !(0..node.child_count()).any(|index| {
                    node.child(index)
                        .is_some_and(|child| rules.code_children.contains(&child.kind()))
                });

            if rules.comments.contains(&kind) {
                exiting = true;
            } else if node.child_count() == 0 || atomic {
                // A container's own `;` between statements is covered by the
                // terminator emitted after the preceding statement.
                let covered_by_terminator = in_container && !node.is_named() && kind == ";";
                let target = if covered_by_terminator {
                    &mut semicolons
                } else {
                    &mut tokens
                };

                if node.end_byte() > node.start_byte() {
                    target.push(Token {
                        bytes: node.byte_range(),
                        start: position(node.start_position()),
                        end: inclusive_end(source, node),
                    });
                }

                exiting = true;
            } else if cursor.goto_first_child() {
                ancestors.push(kind);
            } else {
                exiting = true;
            }

            continue;
        }

        let terminated = node.is_named()
            && !rules.comments.contains(&node.kind())
            && node.end_byte() > node.start_byte()
            && in_container;

        if terminated {
            if tokens.last().is_some_and(|last| {
                !last.is_terminator()
                    && last.bytes.end == node.end_byte()
                    && &source[last.bytes.clone()] == b";"
            }) && let Some(replaced) = tokens.pop()
            {
                semicolons.push(replaced);
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
        } else if cursor.goto_parent() {
            ancestors.pop();
        } else {
            return (tokens, semicolons);
        }
    }
}

/// Where `node` starts and where its last byte is (1-based, byte columns).
/// Unlike the span of its tokens, this keeps a trailing `;` that a statement
/// terminator replaced.
#[must_use]
pub fn node_span(source: &[u8], node: Node<'_>) -> (Position, Position) {
    (position(node.start_position()), inclusive_end(source, node))
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
