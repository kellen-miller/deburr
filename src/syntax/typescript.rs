//! TypeScript and TSX node-kind rules, confirmed against the patched
//! tree-sitter-typescript grammar (the `kellen-miller/tree-sitter-typescript`
//! fork, pinned in `Cargo.toml`). Both dialects share one
//! table; only the grammar differs.
//!
//! A function unit is any function-like node with a `body`, so overload
//! signatures and abstract methods are not units. Control flow has two
//! overlapping sets: `switch_case` adds a decision but no nesting, while
//! `switch_statement` adds nesting but no decision.

use tree_sitter::Node;

use super::{
    BranchShape, Enclosing, FindingRules, LanguageRules, StatementRules, TokenRules, UnitNaming,
};

/// Class-like nodes whose name prefixes the methods inside them.
const CLASS_KINDS: &[&str] = &["class_declaration", "abstract_class_declaration", "class"];

/// Rules table shared by TypeScript and TSX.
pub static RULES: LanguageRules = LanguageRules {
    tokens: TokenRules {
        // The grammar keeps a statement's `;` inside the statement, so an
        // explicit and an automatic semicolon read alike once the terminator
        // replaces it.
        // `case` and `default` bodies are statement lists with no block of
        // their own, so `case 0: return` followed by a newline and `g()` is
        // two statements; the case's `value` is an expression, not one.
        statement_containers: &[
            "program",
            "statement_block",
            "switch_case",
            "switch_default",
        ],
        expression_fields: &["value"],
        atomic: &["string", "template_string", "regex"],
        code_children: &["template_substitution"],
        comments: &["comment", "html_comment"],
    },
    function_kinds: &[
        "function_declaration",
        "generator_function_declaration",
        "function_expression",
        "generator_function",
        "arrow_function",
        "method_definition",
    ],
    decision_kinds: &[
        "if_statement",
        "for_statement",
        "for_in_statement",
        "while_statement",
        "do_statement",
        "catch_clause",
        "ternary_expression",
        // `switch_default` is a different kind and adds nothing.
        "switch_case",
    ],
    decision_weight: None,
    test_regions: None,
    nesting_kinds: &[
        "if_statement",
        "for_statement",
        "for_in_statement",
        "while_statement",
        "do_statement",
        "catch_clause",
        "ternary_expression",
        "switch_statement",
    ],
    chained_branch: None,
    binary_kind: "binary_expression",
    short_circuit_operators: &["&&", "||", "??"],
    naming: UnitNaming::Lexical { name: lexical_name },
    package_name,
    statements: StatementRules {
        comment_kinds: &["comment"],
        block_kind: "statement_block",
        statement_list_kind: None,
        if_kind: "if_statement",
        return_kind: "return_statement",
    },
    // TypeScript has no flat-guard, field-mapping, or assertion signals.
    signals: None,
    ownership: None,
    findings: FindingRules {
        shape: BranchShape::Conditional {
            else_kind: "else_clause",
            parenthesized_kind: "parenthesized_expression",
            statement_kind: None,
        },
        true_kind: "true",
        false_kind: "false",
        comparison_operators: &["===", "!==", "==", "!=", "<", ">", "<=", ">="],
        redundant_boolean_message: "boolean branches return opposite literals; review replacing them with the condition or its negation",
        duplicate_branches_message: "conditional branches contain identical syntax; review consolidating them",
    },
    structure_problem: None,
};

/// Every TypeScript function ID is `typescript/<path>::<name>`.
fn package_name(_root: Node<'_>, _source: &[u8]) -> String {
    "typescript".to_owned()
}

/// `Class.method`, `outer.inner`, `Class.method.nested`: the function's own
/// name, else the name of the node it is assigned to (`const f = () => 1`),
/// prefixed by the classes between it and its enclosing function and then by
/// that function's name. Empty when nothing names it.
///
/// Classes stop at the enclosing function so a method's class is not
/// repeated on the arrows inside it. A nameless function inside a named one
/// is `outer.`, with the trailing dot.
fn lexical_name(node: Node<'_>, source: &[u8], enclosing: Option<Enclosing<'_, '_>>) -> String {
    let own = node
        .child_by_field_name("name")
        .or_else(|| {
            node.parent()
                .and_then(|owner| owner.child_by_field_name("name"))
        })
        .map(|name| text(name, source))
        .unwrap_or_default();
    let mut name = own;
    let mut owner = node.parent();

    while let Some(current) = owner {
        if enclosing.is_some_and(|unit| unit.node == current) {
            break;
        }

        if CLASS_KINDS.contains(&current.kind())
            && let Some(class_name) = current.child_by_field_name("name")
        {
            name = format!("{}.{name}", text(class_name, source));
        }

        owner = current.parent();
    }

    match enclosing {
        Some(unit) => format!("{}.{name}", unit.name),
        None => name,
    }
}

fn text(node: Node<'_>, source: &[u8]) -> String {
    String::from_utf8_lossy(&source[node.byte_range()]).into_owned()
}
