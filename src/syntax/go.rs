//! Go node-kind rules. Kinds were confirmed against tree-sitter-go 0.25's
//! parse trees: `block` holds one `statement_list`, switch cases are direct
//! children of their switch, and an `else if` is an `if_statement` in the
//! previous one's `alternative`.

use tree_sitter::Node;

use super::LanguageRules;
use crate::analysis::FunctionRules;

/// Rules table for Go.
pub static RULES: LanguageRules = LanguageRules;

/// What the function engine needs to know about Go.
///
/// `LanguageRules` is still the unit struct from the scaffold and lives
/// outside this ticket's owned files, so the function table is exported
/// here and selected in `analysis::analyze`.
pub static FUNCTIONS: FunctionRules = FunctionRules {
    function_kinds: &["function_declaration", "method_declaration", "func_literal"],
    // `default_case` is deliberately absent: it adds no decision.
    decision_kinds: &[
        "if_statement",
        "for_statement",
        "expression_case",
        "type_case",
        "communication_case",
    ],
    nesting_kinds: &[
        "if_statement",
        "for_statement",
        "expression_switch_statement",
        "type_switch_statement",
        "select_statement",
    ],
    binary_kind: "binary_expression",
    short_circuit_operators: &["&&", "||"],
    declared_name,
    package_name,
};

/// The name of a declaration: `Name`, or `Receiver.Name` for a method.
/// Function literals have no declared name.
fn declared_name(node: Node<'_>, source: &[u8]) -> Option<String> {
    let name = text(node.child_by_field_name("name")?, source);
    let Some(receiver) = node.child_by_field_name("receiver") else {
        return Some(name);
    };

    let mut cursor = receiver.walk();
    let first = receiver
        .named_children(&mut cursor)
        .find(|child| child.kind() != "comment");

    match first {
        // An empty receiver list names the function like a plain one.
        None => Some(name),
        Some(parameter) => {
            let type_name = parameter.child_by_field_name("type").map_or_else(
                || "receiver".to_owned(),
                |ty| receiver_type_name(ty, source),
            );

            Some(format!("{type_name}.{name}"))
        }
    }
}

/// `T` is `T`, `*T` is `(*T)`, a generic `T[K]` uses `T`, and anything else
/// (parenthesized or qualified types) is `receiver`.
fn receiver_type_name(ty: Node<'_>, source: &[u8]) -> String {
    match ty.kind() {
        "type_identifier" => text(ty, source),
        "pointer_type" => {
            let mut cursor = ty.walk();
            let inner = ty
                .named_children(&mut cursor)
                .find(|child| child.kind() != "comment");

            inner.map_or_else(
                || "receiver".to_owned(),
                |inner| format!("(*{})", receiver_type_name(inner, source)),
            )
        }
        "generic_type" => ty.child_by_field_name("type").map_or_else(
            || "receiver".to_owned(),
            |base| receiver_type_name(base, source),
        ),
        _ => "receiver".to_owned(),
    }
}

/// The identifier of the file's `package_clause`.
fn package_name(root: Node<'_>, source: &[u8]) -> String {
    let mut cursor = root.walk();

    root.named_children(&mut cursor)
        .find(|child| child.kind() == "package_clause")
        .and_then(|clause| {
            let mut inner = clause.walk();

            clause
                .named_children(&mut inner)
                .find(|child| child.kind() == "package_identifier")
        })
        .map_or_else(String::new, |name| text(name, source))
}

fn text(node: Node<'_>, source: &[u8]) -> String {
    String::from_utf8_lossy(&source[node.byte_range()]).into_owned()
}
