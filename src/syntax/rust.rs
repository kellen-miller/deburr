//! Rust node-kind rules, confirmed against tree-sitter-rust 0.24.2.
//!
//! A function unit is a `function_item` with a body or any
//! `closure_expression`; a `function_signature_item` (a trait method with no
//! default body) is not a unit. Names follow rustc's paths: enclosing `mod`
//! names, then `Type::method` in an inherent `impl`, `<Type as Trait>::method`
//! in a trait `impl`, `Trait::method` in a trait's default body, and
//! `parent::{closure#N}` for closures.
//!
//! Macro bodies (`macro_rules!` and the token trees of macro invocations) are
//! not parsed into expressions by the grammar, so code inside them is not
//! measured.

use std::ops::Range;

use tree_sitter::Node;

use super::{
    BranchShape, ChainedBranch, Enclosing, FindingRules, LanguageRules, StatementRules, TokenRules,
    UnitNaming,
};

/// Rules table for Rust.
pub static RULES: LanguageRules = LanguageRules {
    tokens: TokenRules {
        statement_containers: &["source_file", "block", "declaration_list"],
        // The `label` of a labeled block is not a statement.
        expression_fields: &["label"],
        atomic: &["string_literal", "raw_string_literal", "char_literal"],
        code_children: &[],
        comments: &["line_comment", "block_comment"],
    },
    function_kinds: &["function_item", "closure_expression"],
    // `if let` and `while let` are the same kinds as `if` and `while`; `loop`
    // adds nesting only.
    decision_kinds: &[
        "if_expression",
        "while_expression",
        "for_expression",
        "try_expression",
    ],
    decision_weight: Some(decision_weight),
    test_regions: Some(test_regions),
    nesting_kinds: &[
        "if_expression",
        "while_expression",
        "for_expression",
        "loop_expression",
        "match_expression",
    ],
    chained_branch: Some(ChainedBranch {
        clause_kind: "else_clause",
        branch_kind: "if_expression",
    }),
    binary_kind: "binary_expression",
    short_circuit_operators: &["&&", "||"],
    naming: UnitNaming::Lexical { name: unit_name },
    package_name,
    statements: StatementRules {
        comment_kind: "line_comment",
        block_kind: "block",
        statement_list_kind: None,
        if_kind: "if_expression",
        return_kind: "return_expression",
    },
    // Rust's structural signals arrive with its findings.
    signals: None,
    findings: FindingRules {
        shape: BranchShape::Disabled,
        true_kind: "boolean_literal",
        false_kind: "boolean_literal",
        comparison_operators: &["==", "!=", "<", "<=", ">", ">="],
        redundant_boolean_message: "boolean branches return opposite literals; review replacing them with the condition or its negation",
        duplicate_branches_message: "conditional branches contain identical syntax; review consolidating them",
    },
    structure_problem: None,
};

/// Every Rust function ID is `rust/<path>::<name>`.
fn package_name(_root: Node<'_>, _source: &[u8]) -> String {
    "rust".to_owned()
}

/// Decision points beyond the kind table:
///
/// - a `match` arm, unless its whole pattern is the catch-all `_`;
/// - a match guard (`pat if cond`), on top of its arm;
/// - a `let ... else`;
/// - each `&&` joining the conjuncts of an `if let` chain.
///
/// `&&` and `||` between plain expressions are binary expressions and counted
/// by the engine.
fn decision_weight(node: Node<'_>, source: &[u8]) -> u32 {
    match node.kind() {
        "match_arm" => {
            let catch_all = node
                .child_by_field_name("pattern")
                .is_some_and(|pattern| &source[pattern.byte_range()] == b"_");

            u32::from(!catch_all)
        }
        "match_pattern" => u32::from(node.child_by_field_name("condition").is_some()),
        "let_declaration" => u32::from(node.child_by_field_name("alternative").is_some()),
        "let_chain" => {
            let mut cursor = node.walk();
            let conjuncts = node
                .named_children(&mut cursor)
                .filter(|child| !child.kind().ends_with("_comment"))
                .count();

            u32::try_from(conjuncts.saturating_sub(1)).unwrap_or(u32::MAX)
        }
        _ => 0,
    }
}

/// The byte ranges of inline test code, sorted and disjoint. A run of
/// attributes (doc comments may sit between them) applies to the item after
/// it. That item and its attributes are a test region when:
///
/// - an attribute is `cfg(..)` and names `test` outside `not(..)`, which
///   includes `cfg(all(test, ..))` and `cfg(any(test, ..))`; or
/// - the item is a function and an attribute's path ends in `test`
///   (`#[test]`, `#[tokio::test]`, `#[sqlx::test(..)]`).
///
/// A region is never searched for nested regions: it is test code throughout.
fn test_regions(root: Node<'_>, source: &[u8]) -> Vec<Range<usize>> {
    let mut regions = Vec::new();
    let mut pending = vec![root];

    while let Some(parent) = pending.pop() {
        let mut cursor = parent.walk();
        let mut run_start = None;
        let mut cfg_test = false;
        let mut test_function = false;

        for child in parent.named_children(&mut cursor) {
            match child.kind() {
                "attribute_item" => {
                    run_start.get_or_insert(child.start_byte());

                    if let Some(attribute) = child.named_child(0)
                        && let Some(path) = attribute.named_child(0)
                    {
                        let path = &source[path.byte_range()];

                        cfg_test |= path == b"cfg"
                            && attribute
                                .child_by_field_name("arguments")
                                .is_some_and(|arguments| names_test(arguments, source));
                        test_function |= path.rsplit(|&byte| byte == b':').next() == Some(b"test");
                    }
                }
                "line_comment" | "block_comment" => {}
                kind => {
                    if let Some(start) = run_start.take()
                        && (cfg_test || (test_function && kind == "function_item"))
                    {
                        regions.push(start..child.end_byte());
                    } else {
                        pending.push(child);
                    }

                    cfg_test = false;
                    test_function = false;
                }
            }
        }
    }

    regions.sort_by_key(|region| region.start);

    regions
}

/// Whether a `cfg` predicate names `test` anywhere outside a `not(..)`.
fn names_test(predicate: Node<'_>, source: &[u8]) -> bool {
    let mut cursor = predicate.walk();
    let mut negated = false;

    for child in predicate.named_children(&mut cursor) {
        match child.kind() {
            "identifier" => {
                let name = &source[child.byte_range()];

                if name == b"test" {
                    return true;
                }

                negated = name == b"not";
            }
            "token_tree" => {
                if !negated && names_test(child, source) {
                    return true;
                }

                negated = false;
            }
            _ => negated = false,
        }
    }

    false
}

/// The rustc-like path of a function unit: the enclosing unit's name, the
/// `mod`, `impl`, and `trait` containers between it and that unit, then its
/// own name. A closure is `{closure#N}`, N counting the closures its parent
/// owns directly, from 1; a closure in a `const` or `static` initializer
/// counts within that item and is named under it.
///
/// Empty when a function has no name, so the engine falls back to a digest.
fn unit_name(node: Node<'_>, source: &[u8], enclosing: Option<Enclosing<'_, '_>>) -> String {
    let closure = node.kind() == "closure_expression";
    let mut path: Vec<String> = Vec::new();
    // Where closures are counted: the enclosing unit, a `const` or `static`
    // item, or the file.
    let mut scope = None;
    let mut owner = node.parent();

    while let Some(current) = owner {
        if enclosing.is_some_and(|unit| unit.node == current) {
            scope = scope.or(Some(current));

            break;
        }

        match current.kind() {
            "mod_item" | "trait_item" => path.extend(field_text(current, "name", source)),
            "const_item" | "static_item" => {
                scope = scope.or(Some(current));
                path.extend(field_text(current, "name", source));
            }
            "impl_item" => path.push(impl_path(current, source)),
            _ => {}
        }

        owner = current.parent();
    }

    path.reverse();

    if let Some(unit) = enclosing {
        path.insert(0, unit.name.to_owned());
    }

    if closure {
        let scope = scope.unwrap_or_else(|| root_of(node));

        path.push(format!("{{closure#{}}}", closure_ordinal(scope, node)));
    } else if let Some(name) = field_text(node, "name", source) {
        path.push(name);
    } else {
        return String::new();
    }

    path.join("::")
}

/// `Type` for an inherent `impl`, `<Type as Trait>` for a trait `impl`, with
/// generic arguments dropped from both.
fn impl_path(item: Node<'_>, source: &[u8]) -> String {
    let ty = field_text(item, "type", source)
        .map(|text| without_generics(&text))
        .unwrap_or_default();

    match field_text(item, "trait", source) {
        Some(text) => format!("<{ty} as {}>", without_generics(&text)),
        None => ty,
    }
}

/// `text` up to its first `<`, whitespace collapsed. A type that starts with
/// `<` (`<T as Trait>::Output`) is kept whole.
fn without_generics(text: &str) -> String {
    let head = match text.find('<') {
        Some(index) if index > 0 => &text[..index],
        _ => text,
    };

    head.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn field_text(node: Node<'_>, field: &str, source: &[u8]) -> Option<String> {
    node.child_by_field_name(field)
        .map(|child| String::from_utf8_lossy(&source[child.byte_range()]).into_owned())
}

fn root_of(node: Node<'_>) -> Node<'_> {
    let mut root = node;

    while let Some(parent) = root.parent() {
        root = parent;
    }

    root
}

/// The 1-based position of `target` among the closures `scope` owns directly,
/// in source order. A closure's own closures, the contents of nested `fn`
/// items, and the initializers of `const` and `static` items belong to them,
/// so the walk does not enter any of those.
fn closure_ordinal(scope: Node<'_>, target: Node<'_>) -> usize {
    let mut cursor = scope.walk();
    let mut pending: Vec<Node<'_>> = scope.named_children(&mut cursor).collect();
    let mut count = 0;

    pending.reverse();

    while let Some(node) = pending.pop() {
        match node.kind() {
            "closure_expression" => {
                count += 1;

                if node == target {
                    return count;
                }
            }
            "function_item" | "const_item" | "static_item" => {}
            _ => {
                let mut inner = node.walk();
                let children: Vec<Node<'_>> = node.named_children(&mut inner).collect();

                pending.extend(children.into_iter().rev());
            }
        }
    }

    count
}
