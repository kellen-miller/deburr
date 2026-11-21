//! Go node-kind rules. Kinds were confirmed against tree-sitter-go 0.25's
//! parse trees: `block` holds one `statement_list`, switch cases are direct
//! children of their switch, and an `else if` is an `if_statement` in the
//! previous one's `alternative`.

use tree_sitter::{Node, Point};

use super::{
    BranchShape, CallTarget, FindingRules, LanguageRules, SignalRules, StatementRules, TokenRules,
    UnitNaming,
};

/// Rules table for Go.
pub static RULES: LanguageRules = LanguageRules {
    tokens: TokenRules {
        statement_containers: &["statement_list"],
        expression_fields: &[],
        atomic: &[
            "interpreted_string_literal",
            "raw_string_literal",
            "rune_literal",
        ],
        code_children: &[],
        comments: &["comment"],
    },
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
    naming: UnitNaming::Positional { declared_name },
    package_name,
    statements: StatementRules {
        comment_kind: "comment",
        block_kind: "block",
        statement_list_kind: Some("statement_list"),
        if_kind: "if_statement",
        return_kind: "return_statement",
    },
    signals: Some(SignalRules {
        // `goto` and `fallthrough` are different kinds and do not guard.
        guard_kinds: &["return_statement", "break_statement", "continue_statement"],
        composite_kind: "composite_literal",
        // Array, slice, map, and generic literal types are not mappings.
        composite_type_kinds: &["type_identifier", "qualified_type", "struct_type"],
        keyed_element_kind: "keyed_element",
        element_kind: "literal_element",
        // tree-sitter-go gives `true`, `false`, `nil`, and `iota` their own
        // kinds where the Go AST has plain identifiers.
        mapping_key_kinds: &["identifier", "true", "false", "nil", "iota"],
        mapping_value_kinds: &[
            "identifier",
            "selector_expression",
            "true",
            "false",
            "nil",
            "iota",
        ],
        // A generic call with two or more type arguments, `f[A, B](x)`,
        // parses as a conversion.
        call_kinds: &["call_expression", "type_conversion_expression"],
        call_target,
        assertion_prefixes: &["assert", "require", "expect"],
        assertion_receivers: &["assert", "require", "testify"],
        testing_receivers: &["t", "tb", "tt", "test", "testingt"],
        testing_methods: &["error", "errorf", "fatal", "fatalf", "fail", "failnow"],
    }),
    findings: FindingRules {
        shape: BranchShape::Sequenced {
            redundant_owner_kinds: &["function_declaration", "method_declaration"],
            returns_plain_bool,
        },
        true_kind: "true",
        false_kind: "false",
        comparison_operators: &["==", "!=", "<", ">", "<=", ">="],
        redundant_boolean_message: "boolean branches return opposite literals; review replacing them with the condition or its negation",
        duplicate_branches_message: "if branches have identical syntax; review whether the condition and branch effects can be simplified",
    },
    structure_problem: Some(structure_problem),
};

/// The name and selector receiver of a call.
fn call_target(call: Node<'_>, source: &[u8]) -> Option<CallTarget> {
    let callee = match call.kind() {
        "call_expression" => call.child_by_field_name("function")?,
        _ => call.child_by_field_name("type")?,
    };
    let receiver = (call.kind() == "call_expression" && callee.kind() == "selector_expression")
        .then(|| callee.child_by_field_name("operand"))
        .flatten()
        .filter(|operand| operand.kind() == "identifier")
        .map(|operand| text(operand, source));

    Some(CallTarget {
        name: callee_name(callee, source)?,
        receiver,
    })
}

/// `f` and `x.f` are named `f`; instantiations are named for what they
/// instantiate; anything else (a call of a call, a literal) has no name.
fn callee_name(callee: Node<'_>, source: &[u8]) -> Option<String> {
    match callee.kind() {
        "identifier" => Some(text(callee, source)),
        "selector_expression" => callee
            .child_by_field_name("field")
            .map(|field| text(field, source)),
        "qualified_type" => callee
            .child_by_field_name("name")
            .map(|name| text(name, source)),
        "index_expression" => callee_name(callee.child_by_field_name("operand")?, source),
        "generic_type" => callee_name(callee.child_by_field_name("type")?, source),
        "type_identifier" => Some(text(callee, source)),
        _ => None,
    }
}

/// One unnamed result of type `bool`, written `bool` or `(bool)`.
fn returns_plain_bool(function: Node<'_>, source: &[u8]) -> bool {
    let Some(result) = function.child_by_field_name("result") else {
        return false;
    };

    let ty = if result.kind() == "parameter_list" {
        let mut cursor = result.walk();
        let mut parameters = result
            .named_children(&mut cursor)
            .filter(|child| child.kind() != "comment");

        match (parameters.next(), parameters.next()) {
            (Some(parameter), None) if parameter.child_by_field_name("name").is_none() => {
                parameter.child_by_field_name("type")
            }
            _ => None,
        }
    } else {
        Some(result)
    };

    ty.is_some_and(|ty| ty.kind() == "type_identifier" && text(ty, source) == "bool")
}

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

/// What `go/parser` rejects and tree-sitter-go accepts: top-level statements,
/// a missing package clause, and invalid string or rune literals. Returns the
/// earliest offending position.
fn structure_problem(root: Node<'_>, source: &[u8]) -> Option<Point> {
    match (top_level_problem(root), literal_problem(root, source)) {
        (Some(top), Some(literal)) => Some(top.min(literal)),
        (top, literal) => top.or(literal),
    }
}

/// The first top-level node that is not a declaration, or the start of the
/// file when it has no package clause.
fn top_level_problem(root: Node<'_>) -> Option<Point> {
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

/// The start of the first interpreted string or rune literal whose escapes
/// or length `go/parser` rejects, in source order.
fn literal_problem(root: Node<'_>, source: &[u8]) -> Option<Point> {
    let mut cursor = root.walk();

    loop {
        let node = cursor.node();
        let quote = match node.kind() {
            "interpreted_string_literal" => Some('"'),
            "rune_literal" => Some('\''),
            _ => None,
        };

        if let Some(quote) = quote {
            let invalid = source
                .get(node.start_byte() + 1..node.end_byte().saturating_sub(1))
                .is_none_or(|body| invalid_literal_body(body, quote));

            if invalid {
                return Some(node.start_position());
            }
        } else if cursor.goto_first_child() {
            continue;
        }

        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return None;
            }
        }
    }
}

/// Whether the text between a literal's quotes is invalid. A rune literal
/// must also hold exactly one character or escape.
fn invalid_literal_body(body: &[u8], quote: char) -> bool {
    let Ok(mut rest) = std::str::from_utf8(body) else {
        return true;
    };
    let mut units = 0_usize;

    while let Some(next) = rest.chars().next() {
        units += 1;

        if next == '\\' {
            let Some(length) = escape_length(&rest[1..], quote) else {
                return true;
            };

            rest = &rest[1 + length..];
        } else if next == '\n' {
            // A raw newline ends a rune literal early; strings cannot hold one.
            return true;
        } else {
            rest = &rest[next.len_utf8()..];
        }
    }

    quote == '\'' && units != 1
}

/// The byte length of the valid escape that follows a backslash, or `None`
/// when `go/parser` rejects it. `\'` is valid only in runes and `\"` only in
/// strings; octal escapes need three digits and at most `\377`; `\u` and `\U`
/// must name a Unicode scalar value (no surrogates, nothing above U+10FFFF).
fn escape_length(escape: &str, quote: char) -> Option<usize> {
    let first = escape.chars().next()?;
    let (prefix, digit_count, radix) = match first {
        'a' | 'b' | 'f' | 'n' | 'r' | 't' | 'v' | '\\' => return Some(1),
        '\'' | '"' => return (first == quote).then_some(1),
        '0'..='7' => (0, 3, 8),
        'x' => (1, 2, 16),
        'u' => (1, 4, 16),
        'U' => (1, 8, 16),
        _ => return None,
    };

    let digits = escape.get(prefix..prefix + digit_count)?;

    if !digits.chars().all(|digit| digit.is_digit(radix)) {
        return None;
    }

    let value = u32::from_str_radix(digits, radix).ok()?;
    let valid = match first {
        '0'..='7' => value <= 0o377,
        'u' | 'U' => char::from_u32(value).is_some(),
        _ => true,
    };

    valid.then_some(prefix + digit_count)
}
