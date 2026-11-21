//! Findings found while walking a function body: `redundant-boolean-return`
//! and `duplicate-branches`, by each language's branch shape.

use tree_sitter::Node;

use super::{
    DUPLICATE_BRANCHES, OwnerContext, REDUNDANT_BOOLEAN_RETURN, only_child, statements,
    tokens_between,
};
use crate::identity;
use crate::report::{Detail, Position};
use crate::syntax::{BranchShape, LanguageRules, Syntax, Token, node_span};

/// A finding found in a body, before its owner's identity is known.
pub(super) struct Candidate {
    pub(super) rule: &'static str,
    pub(super) message: &'static str,
    pub(super) start: Position,
    pub(super) end: Position,
    /// [`identity::token_content`] of the flagged code.
    pub(super) content: Vec<u8>,
    pub(super) details: Vec<Detail>,
}

/// The first and last real token's positions.
fn span_of(tokens: &[Token]) -> Option<(Position, Position)> {
    let mut real = tokens.iter().filter(|token| !token.is_terminator());
    let first = real.next()?;
    let last = real.next_back().unwrap_or(first);

    Some((first.start, last.end))
}

/// The findings that `node` starts, by the language's branch shape.
pub(super) fn branch_candidates(
    node: Node<'_>,
    owner: &OwnerContext<'_>,
    syntax: &Syntax,
    rules: &LanguageRules,
) -> Vec<Candidate> {
    let kind = node.kind();

    match &rules.findings.shape {
        BranchShape::Sequenced { .. } if kind == rules.statements.if_kind => {
            duplicate_branches(node, owner, syntax, rules)
                .into_iter()
                .collect()
        }
        BranchShape::Sequenced { .. }
            if kind == rules.statements.block_kind && owner.sequenced_returns =>
        {
            redundant_boolean_returns(node, owner, syntax, rules)
        }
        BranchShape::Conditional {
            else_kind,
            parenthesized_kind,
            statement_kind,
        } if kind == rules.statements.if_kind => conditional_finding(
            node,
            else_kind,
            parenthesized_kind,
            *statement_kind,
            owner,
            syntax,
            rules,
        )
        .into_iter()
        .collect(),
        _ => Vec::new(),
    }
}

/// `duplicate-branches`: an `if` with a plain `else` block whose token text
/// equals the `then` block's. An `else if` is not a duplicate branch.
fn duplicate_branches(
    node: Node<'_>,
    owner: &OwnerContext<'_>,
    syntax: &Syntax,
    rules: &LanguageRules,
) -> Option<Candidate> {
    let consequence = node.child_by_field_name("consequence")?;
    let alternative = node
        .child_by_field_name("alternative")
        .filter(|alternative| alternative.kind() == rules.statements.block_kind)?;

    if statements(consequence, rules).is_empty() || statements(alternative, rules).is_empty() {
        return None;
    }

    let content = identity::token_content(
        syntax,
        tokens_between(syntax, consequence.start_byte(), consequence.end_byte()),
    );
    let other = identity::token_content(
        syntax,
        tokens_between(syntax, alternative.start_byte(), alternative.end_byte()),
    );

    if content != other {
        return None;
    }

    let (start, end) = span_of(tokens_between(syntax, node.start_byte(), node.end_byte()))?;

    Some(Candidate {
        rule: DUPLICATE_BRANCHES,
        message: rules.findings.duplicate_branches_message,
        start,
        end,
        content,
        details: vec![
            detail("owner", owner.name),
            detail("candidate_only", "true"),
            detail("preserve_condition", "true"),
        ],
    })
}

/// `redundant-boolean-return` over the statements of one block: a comparison
/// `if` with no initializer or `else` that returns one boolean literal,
/// followed directly by a return of the opposite literal. Case-clause bodies
/// are not blocks, so they are never scanned.
fn redundant_boolean_returns(
    block: Node<'_>,
    owner: &OwnerContext<'_>,
    syntax: &Syntax,
    rules: &LanguageRules,
) -> Vec<Candidate> {
    let list = statements(block, rules);

    list.windows(2)
        .filter_map(|pair| redundant_boolean_pair(pair[0], pair[1], owner, syntax, rules))
        .collect()
}

fn redundant_boolean_pair(
    conditional: Node<'_>,
    following: Node<'_>,
    owner: &OwnerContext<'_>,
    syntax: &Syntax,
    rules: &LanguageRules,
) -> Option<Candidate> {
    if conditional.kind() != rules.statements.if_kind
        || conditional.child_by_field_name("initializer").is_some()
        || conditional.child_by_field_name("alternative").is_some()
    {
        return None;
    }

    let inner = statements(conditional.child_by_field_name("consequence")?, rules);

    if inner.len() != 1 {
        return None;
    }

    let then_value = boolean_return(inner[0], &syntax.source, rules)?;
    let following_value = boolean_return(following, &syntax.source, rules)?;

    if then_value == following_value {
        return None;
    }

    let condition = conditional.child_by_field_name("condition")?;
    let comparison = condition.kind() == rules.binary_kind
        && condition
            .child_by_field_name("operator")
            .is_some_and(|operator| {
                rules
                    .findings
                    .comparison_operators
                    .contains(&operator.kind())
            });

    if !comparison {
        return None;
    }

    let tokens = tokens_between(syntax, conditional.start_byte(), following.end_byte());
    let (start, end) = span_of(tokens)?;
    let condition_text = String::from_utf8_lossy(&syntax.source[condition.byte_range()]);

    Some(Candidate {
        rule: REDUNDANT_BOOLEAN_RETURN,
        message: rules.findings.redundant_boolean_message,
        start,
        end,
        content: identity::token_content(syntax, tokens),
        details: vec![
            detail("owner", owner.name),
            detail(
                "condition",
                &condition_text
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            detail("then_return", &then_value.to_string()),
            detail("else_return", &following_value.to_string()),
            detail("candidate_only", "true"),
        ],
    })
}

/// The literal returned by `statement`, if it is `return true` or
/// `return false` and nothing else.
fn boolean_return(statement: Node<'_>, source: &[u8], rules: &LanguageRules) -> Option<bool> {
    if statement.kind() != rules.statements.return_kind {
        return None;
    }

    let results = only_child(statement, rules)?;
    let value = only_child(results, rules)?;

    boolean_literal(value, source, rules)
}

/// The value of a boolean literal. A language with one literal kind for both
/// values (Rust's `boolean_literal`) is told apart by the literal's text.
fn boolean_literal(node: Node<'_>, source: &[u8], rules: &LanguageRules) -> Option<bool> {
    let findings = &rules.findings;
    let kind = node.kind();

    if kind != findings.true_kind && kind != findings.false_kind {
        None
    } else if findings.true_kind == findings.false_kind {
        Some(&source[node.byte_range()] == b"true")
    } else {
        Some(kind == findings.true_kind)
    }
}

/// A TypeScript or Rust `if` and the branch after it, which is its `else` or,
/// with none, the next statement in the same block. Looks only at the `if`,
/// never at the code around it, so the finding fires at most once per `if`:
///
/// - `redundant-boolean-return` when each branch is a lone `return true` or
///   `return false` (bare or in a block; in Rust also a lone `true` or
///   `false` tail expression), the literals differ, and the condition, minus
///   one pair of parentheses, is a comparison;
/// - otherwise `duplicate-branches` when the `then` branch is a non-empty
///   block with the same token text as an `else` block. Equality needs a
///   braced `then` branch, so a bare statement branch is never a duplicate
///   and needs no statement terminator of its own.
///
/// `statement_kind` is the node a language wraps around an expression used
/// as a statement; the `if` and a branch's `return` sit inside one. The range
/// runs to the end of the other branch when that is a following statement.
/// Neither finding carries more detail than its owner.
fn conditional_finding(
    node: Node<'_>,
    else_kind: &str,
    parenthesized_kind: &str,
    statement_kind: Option<&str>,
    owner: &OwnerContext<'_>,
    syntax: &Syntax,
    rules: &LanguageRules,
) -> Option<Candidate> {
    let consequence = node.child_by_field_name("consequence")?;
    let alternative = match node.child_by_field_name("alternative") {
        Some(clause) if clause.kind() == else_kind => {
            let mut cursor = clause.walk();

            clause.named_children(&mut cursor).last().unwrap_or(clause)
        }
        Some(branch) => branch,
        None => {
            let statement = node
                .parent()
                .filter(|parent| statement_kind == Some(parent.kind()))
                .unwrap_or(node);
            let in_block = statement
                .parent()
                .is_some_and(|parent| parent.kind() == rules.statements.block_kind);
            let mut next = statement.next_named_sibling();

            while next
                .is_some_and(|sibling| rules.statements.comment_kinds.contains(&sibling.kind()))
            {
                next = next.and_then(|sibling| sibling.next_named_sibling());
            }

            next.filter(|_| in_block)?
        }
    };

    let literal = |branch: Node<'_>| {
        let mut statement = if branch.kind() == rules.statements.block_kind {
            let mut inner = statements(branch, rules).into_iter();

            inner.next().filter(|_| inner.next().is_none())?
        } else {
            branch
        };

        // `return true;` is a `return` inside an expression statement.
        if statement_kind == Some(statement.kind()) {
            statement = only_child(statement, rules)
                .filter(|inner| inner.kind() == rules.statements.return_kind)?;
        }

        // A tail expression is the literal itself.
        if statement.kind() != rules.statements.return_kind {
            return boolean_literal(statement, &syntax.source, rules);
        }

        if statement.named_child_count() != 1 {
            return None;
        }

        boolean_literal(statement.named_child(0)?, &syntax.source, rules)
    };

    let mut condition = node.child_by_field_name("condition");

    if let Some(wrapped) = condition.filter(|wrapper| wrapper.kind() == parenthesized_kind)
        && wrapped.named_child_count() == 1
    {
        condition = wrapped.named_child(0);
    }

    let comparison = condition.is_some_and(|condition| {
        condition.kind() == rules.binary_kind
            && condition
                .child_by_field_name("operator")
                .is_some_and(|operator| {
                    rules
                        .findings
                        .comparison_operators
                        .contains(&operator.kind())
                })
    });
    let opposite_returns = matches!(
        (literal(consequence), literal(alternative)),
        (Some(then_value), Some(other_value)) if then_value != other_value
    );

    let tokens = |branch: Node<'_>| {
        identity::token_content(
            syntax,
            tokens_between(syntax, branch.start_byte(), branch.end_byte()),
        )
    };
    let (rule, message) = if opposite_returns && comparison {
        (
            REDUNDANT_BOOLEAN_RETURN,
            rules.findings.redundant_boolean_message,
        )
    } else if consequence.kind() == rules.statements.block_kind
        && alternative.kind() == rules.statements.block_kind
        && !statements(consequence, rules).is_empty()
        && tokens(consequence) == tokens(alternative)
    {
        (
            DUPLICATE_BRANCHES,
            rules.findings.duplicate_branches_message,
        )
    } else {
        return None;
    };

    let (start, mut end) = node_span(&syntax.source, node);
    let mut content = tokens(node);

    if alternative.end_byte() > node.end_byte() {
        end = node_span(&syntax.source, alternative).1;
        content.extend(tokens(alternative));
    }

    Some(Candidate {
        rule,
        message,
        start,
        end,
        content,
        details: vec![
            detail("owner", owner.name),
            detail("candidate_only", "true"),
        ],
    })
}

fn detail(key: &str, value: &str) -> Detail {
    Detail {
        key: key.to_owned(),
        value: value.to_owned(),
    }
}
