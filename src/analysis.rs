//! Turns a parsed file into function units, findings, and a per-line code map.
//!
//! The engine is language independent: a [`LanguageRules`] table names the
//! node kinds that matter, plus a few shape functions per language.

use std::collections::HashMap;
use std::ops::Range;

use tree_sitter::Node;

mod findings;

use self::findings::{Candidate, branch_candidates};
use crate::identity::{self, FindingSketch, UnitSketch};
use crate::report::{
    Category, Finding, Function, HIGH_COMPLEXITY_THRESHOLD, Position, StructuralSignals,
};
use crate::syntax::{
    BranchShape, CallShape, Enclosing, LanguageRules, OwnershipRules, SignalRules, Syntax, Token,
    UnitNaming, in_regions,
};

/// Rule ID of an `if` that returns opposite boolean literals.
pub const REDUNDANT_BOOLEAN_RETURN: &str = "redundant-boolean-return";

/// Rule ID of an `if` whose two branches have identical syntax.
pub const DUPLICATE_BRANCHES: &str = "duplicate-branches";

/// Everything measured in one file.
#[derive(Debug, Default)]
pub struct FileAnalysis {
    pub functions: Vec<Function>,
    /// The byte range of each function's body, keyed by function ID, so clone
    /// detection can tell body code from a signature line.
    pub bodies: Vec<FunctionBody>,
    pub findings: Vec<Finding>,
    /// Index `n - 1` is true when line `n` holds code.
    pub code_lines: Vec<bool>,
    /// Code lines that hold inline test code (see [`LanguageRules::test_regions`]).
    pub inline_test_code_lines: u32,
    /// The byte ranges of inline test code, sorted and disjoint. Empty for a
    /// file whose category is not production.
    pub test_regions: Vec<Range<usize>>,
}

/// Where one function's body sits in the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionBody {
    pub function_id: String,
    pub bytes: std::ops::Range<usize>,
}

/// One discovered function unit and what has been measured so far.
struct Unit<'tree> {
    node: Node<'tree>,
    /// Index of the innermost enclosing unit.
    parent: Option<usize>,
    start: Position,
    end: Position,
    name: String,
    /// A declaration with its own name rather than a literal.
    declared: bool,
    /// `sha256:<hex>` over the unit's token text.
    fingerprint: String,
    sloc: u32,
    complexity: Complexity,
    candidates: Vec<Candidate>,
}

/// What the walk over one function body measures.
#[derive(Debug, Default)]
struct Complexity {
    cyclomatic: u32,
    max_nesting: u32,
    signals: StructuralSignals,
}

/// Analyzes one parsed file.
///
/// In a production file, functions inside the language's test regions are
/// category `test`, and their code lines are counted as inline test lines.
/// With `include_inline_tests` false those functions and their findings are
/// left out; identities are still derived from every function, so they do not
/// depend on the flag.
#[must_use]
pub fn analyze(
    path: &str,
    category: Category,
    syntax: &Syntax,
    include_inline_tests: bool,
) -> FileAnalysis {
    let code_lines = syntax.code_lines();
    let rules = syntax.language.rules();
    let test_regions = match rules.test_regions {
        Some(regions) if category == Category::Production => {
            regions(syntax.tree.root_node(), &syntax.source)
        }
        _ => Vec::new(),
    };
    let inline_test_code_lines = syntax
        .region_code_lines(&test_regions)
        .iter()
        .zip(&code_lines)
        .filter(|&(&inline, &code)| inline && code)
        .count();
    let mut units = discover_units(syntax, rules);

    name_units(&mut units, syntax, rules);

    let sketches: Vec<UnitSketch<'_>> = units
        .iter()
        .map(|unit| UnitSketch {
            name: &unit.name,
            parent: unit.parent,
            declaration: unit.declared,
            fingerprint: &unit.fingerprint,
        })
        .collect();
    let identities = match rules.naming {
        UnitNaming::Positional { .. } => identity::assign_unit_identities(&sketches),
        UnitNaming::Lexical { .. } => identity::assign_lexical_identities(&sketches),
    };

    assign_sloc(&mut units, &code_lines);

    let inline_tests: Vec<bool> = units
        .iter()
        .map(|unit| in_regions(&test_regions, unit.node.start_byte()))
        .collect();

    for (unit, &inline_test) in units.iter_mut().zip(&inline_tests) {
        if inline_test && !include_inline_tests {
            continue;
        }

        if let Some(body) = unit.node.child_by_field_name("body") {
            let owner = OwnerContext {
                name: &unit.name,
                sequenced_returns: match &rules.findings.shape {
                    BranchShape::Sequenced {
                        redundant_owner_kinds,
                        returns_plain_bool,
                    } => {
                        redundant_owner_kinds.contains(&unit.node.kind())
                            && returns_plain_bool(unit.node, &syntax.source)
                    }
                    BranchShape::Conditional { .. } => false,
                },
                test_file: category == Category::Test || inline_test,
            };
            let scan = scan_body(body, &owner, syntax, rules);

            unit.complexity = scan.complexity;
            unit.candidates = scan.candidates;
        }
    }

    let package = (rules.package_name)(syntax.tree.root_node(), &syntax.source);
    let mut findings = Vec::new();
    let mut functions = Vec::with_capacity(units.len());
    let mut bodies = Vec::with_capacity(units.len());

    for ((unit, identity), inline_test) in units.into_iter().zip(identities).zip(inline_tests) {
        if inline_test && !include_inline_tests {
            continue;
        }

        let mut occurrences: HashMap<&str, usize> = HashMap::new();

        for candidate in unit.candidates {
            let occurrence = occurrences.entry(candidate.rule).or_default();
            *occurrence += 1;

            let sketch = FindingSketch {
                path,
                rule: candidate.rule,
                owner_identity: &identity.name,
                owner_fingerprint: &unit.fingerprint,
                content: &candidate.content,
                occurrence: *occurrence,
            };

            findings.push(Finding {
                id: identity::finding_id(&sketch),
                fingerprint: identity::finding_fingerprint(&sketch),
                rule: candidate.rule.to_owned(),
                path: path.to_owned(),
                severity: "review".to_owned(),
                message: candidate.message.to_owned(),
                details: candidate.details,
                start: candidate.start,
                end: candidate.end,
                identity_ambiguous: identity.ambiguous,
            });
        }

        let mass = if unit.sloc == 0 {
            0.0
        } else {
            f64::from(unit.complexity.cyclomatic) * f64::from(unit.sloc).sqrt()
        };

        let id = identity::function_id(&package, path, &identity.name);

        bodies.push(FunctionBody {
            function_id: id.clone(),
            bytes: unit
                .node
                .child_by_field_name("body")
                .map_or_else(|| unit.node.byte_range(), |body| body.byte_range()),
        });
        functions.push(Function {
            id,
            fingerprint: unit.fingerprint,
            path: path.to_owned(),
            category: if inline_test {
                Category::Test
            } else {
                category
            },
            name: unit.name,
            structural_signals: unit.complexity.signals,
            start: unit.start,
            end: unit.end,
            sloc: unit.sloc,
            cyclomatic: unit.complexity.cyclomatic,
            max_nesting: unit.complexity.max_nesting,
            mass,
            high_complexity: unit.complexity.cyclomatic > HIGH_COMPLEXITY_THRESHOLD,
            identity_ambiguous: identity.ambiguous,
        });
    }

    FileAnalysis {
        functions,
        bodies,
        findings,
        code_lines,
        inline_test_code_lines: u32::try_from(inline_test_code_lines).unwrap_or(u32::MAX),
        test_regions,
    }
}

/// The tokens that start inside `[start, end)`. A statement terminator that
/// starts exactly at `end` belongs to the enclosing statement, so the strict
/// comparison leaves it out.
fn tokens_between(syntax: &Syntax, start: usize, end: usize) -> &[Token] {
    let first = syntax
        .tokens
        .partition_point(|token| token.bytes.start < start);
    let last = syntax
        .tokens
        .partition_point(|token| token.bytes.start < end);

    &syntax.tokens[first..last]
}

/// Finds every function unit in pre-order, so units come out sorted by start
/// (outer before inner) and a unit's parent always precedes it.
fn discover_units<'tree>(syntax: &'tree Syntax, rules: &LanguageRules) -> Vec<Unit<'tree>> {
    let root = syntax.tree.root_node();
    let mut units: Vec<Unit<'tree>> = Vec::new();
    let mut pending: Vec<(Node<'tree>, Option<usize>)> = vec![(root, None)];
    let mut cursor = root.walk();

    while let Some((node, enclosing)) = pending.pop() {
        let mut owner = enclosing;

        if rules.function_kinds.contains(&node.kind()) && node.child_by_field_name("body").is_some()
        {
            let tokens = tokens_between(syntax, node.start_byte(), node.end_byte());

            if let (Some(head), Some(tail)) = (tokens.first(), tokens.last()) {
                owner = Some(units.len());
                units.push(Unit {
                    node,
                    parent: enclosing,
                    start: head.start,
                    end: tail.end,
                    name: String::new(),
                    declared: false,
                    fingerprint: identity::token_fingerprint(syntax, tokens),
                    sloc: 0,
                    complexity: Complexity {
                        cyclomatic: 1,
                        ..Complexity::default()
                    },
                    candidates: Vec::new(),
                });
            }
        }

        let children: Vec<Node<'tree>> = node.named_children(&mut cursor).collect();
        pending.extend(children.into_iter().rev().map(|child| (child, owner)));
    }

    units
}

/// Names units. Positional languages use a declaration's own name, else
/// `file.funcN` (N counts every top-level unit) or `Parent.funcN` (N counts
/// the parent's direct literals). Lexical languages compose the name from the
/// enclosing classes and unit, and give a unit with no name at all
/// `anon-<digest>`. A parent always precedes its children.
fn name_units(units: &mut [Unit<'_>], syntax: &Syntax, rules: &LanguageRules) {
    let mut top_level = 0_usize;
    let mut literals = vec![0_usize; units.len()];

    for index in 0..units.len() {
        match rules.naming {
            UnitNaming::Positional { declared_name } => {
                let declared = declared_name(units[index].node, &syntax.source);

                units[index].declared = declared.is_some();
                units[index].name = match (declared, units[index].parent) {
                    (Some(name), _) => name,
                    (None, None) => format!("file.func{}", top_level + 1),
                    (None, Some(parent)) => {
                        literals[parent] += 1;

                        format!("{}.func{}", units[parent].name, literals[parent])
                    }
                };
            }
            UnitNaming::Lexical { name } => {
                let enclosing = units[index].parent.map(|parent| Enclosing {
                    node: units[parent].node,
                    name: &units[parent].name,
                });
                let composed = name(units[index].node, &syntax.source, enclosing);
                let fingerprint = &units[index].fingerprint;
                let anonymous = format!(
                    "anon-{}",
                    fingerprint.strip_prefix("sha256:").unwrap_or(fingerprint)
                );

                units[index].name = if composed.is_empty() {
                    anonymous
                } else {
                    composed
                };
            }
        }

        if units[index].parent.is_none() {
            top_level += 1;
        }
    }
}

/// Gives each code line to the innermost unit whose line range contains it:
/// the smallest byte span, the earlier unit winning a tie. Lines outside
/// every unit count nowhere, and a nested unit's lines never reach its parent.
fn assign_sloc(units: &mut [Unit<'_>], code_lines: &[bool]) {
    let mut owners: Vec<Option<usize>> = vec![None; code_lines.len()];

    for (index, unit) in units.iter().enumerate() {
        let span = unit.node.byte_range().len();
        let first = (unit.start.line as usize).saturating_sub(1);
        let last = (unit.end.line as usize).min(code_lines.len());

        for line in first..last {
            let innermost =
                owners[line].is_none_or(|current| span < units[current].node.byte_range().len());

            if code_lines[line] && innermost {
                owners[line] = Some(index);
            }
        }
    }

    for owner in owners.into_iter().flatten() {
        units[owner].sloc += 1;
    }
}

/// What the body walk needs to know about the function that owns the body.
struct OwnerContext<'a> {
    /// Display name, recorded in finding details.
    name: &'a str,
    /// Whether blocks in this body are scanned for an `if` followed by a
    /// returned boolean literal (sequenced branch shapes only).
    sequenced_returns: bool,
    test_file: bool,
}

/// What one body walk found.
struct BodyScan {
    complexity: Complexity,
    candidates: Vec<Candidate>,
}

/// Measures one function body in a single pre-order walk: cyclomatic
/// complexity, nesting, every structural signal, and finding candidates.
/// Nested function units are skipped: their contents belong to them.
fn scan_body(
    body: Node<'_>,
    owner: &OwnerContext<'_>,
    syntax: &Syntax,
    rules: &LanguageRules,
) -> BodyScan {
    let mut complexity = Complexity {
        cyclomatic: 1,
        ..Complexity::default()
    };
    let mut candidates = Vec::new();

    if let Some(signals) = &rules.signals {
        complexity.signals.flat_guards = statements(body, rules)
            .into_iter()
            .filter(|statement| is_guard_clause(*statement, signals, rules))
            .count()
            .try_into()
            .unwrap_or(u32::MAX);
    }

    let mut cursor = body.walk();
    let mut pending = vec![(body, 0_u32)];

    while let Some((node, mut depth)) = pending.pop() {
        let kind = node.kind();

        if rules.function_kinds.contains(&kind) {
            continue;
        }

        let short_circuit = kind == rules.binary_kind
            && node
                .child_by_field_name("operator")
                .is_some_and(|operator| rules.short_circuit_operators.contains(&operator.kind()));

        if short_circuit || rules.decision_kinds.contains(&kind) {
            complexity.cyclomatic += 1;
        }

        if let Some(weight) = rules.decision_weight {
            complexity.cyclomatic += weight(node, &syntax.source);
        }

        // An `else if` sits one level inside the `if` that heads its chain,
        // but it is not nested deeper than that head.
        if let Some(chain) = &rules.chained_branch
            && kind == chain.branch_kind
            && node
                .parent()
                .is_some_and(|parent| parent.kind() == chain.clause_kind)
        {
            depth = depth.saturating_sub(1);
        }

        let mut inner = depth;

        if rules.nesting_kinds.contains(&kind) {
            if depth > 0 {
                complexity.signals.nested_branches += 1;
            }

            inner += 1;
            complexity.max_nesting = complexity.max_nesting.max(inner);
        }

        if let Some(signals) = &rules.signals {
            if kind == signals.composite_kind {
                complexity.signals.field_mappings += field_mappings(node, signals, rules);
            }

            if owner.test_file
                && signals.call_kinds.contains(&kind)
                && is_assertion_call(node, signals, &syntax.source)
            {
                complexity.signals.assertion_like_calls += 1;
            }
        }

        if let Some(ownership) = &rules.ownership {
            count_ownership(node, ownership, syntax, rules, &mut complexity.signals);
        }

        candidates.extend(branch_candidates(node, owner, syntax, rules));

        let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
        pending.extend(children.into_iter().rev().map(|child| (child, inner)));
    }

    BodyScan {
        complexity,
        candidates,
    }
}

/// Adds `node` to the ownership and panic signals when it is a method call
/// with a listed name and argument count, a panic macro, or an `unsafe`
/// block. A call on a path (`Vec::clone(&v)`) is not a method call.
fn count_ownership(
    node: Node<'_>,
    ownership: &OwnershipRules,
    syntax: &Syntax,
    rules: &LanguageRules,
    signals: &mut StructuralSignals,
) {
    let kind = node.kind();

    if kind == ownership.unsafe_kind {
        signals.unsafe_blocks += 1;
    } else if kind == ownership.macro_kind {
        // The last segment of the macro's path: `panic` in `std::panic!`.
        let name = node
            .child_by_field_name("macro")
            .map(|path| path.child_by_field_name("name").unwrap_or(path));

        if name.is_some_and(|name| {
            ownership
                .panic_macros
                .iter()
                .any(|panic| syntax.source[name.byte_range()] == *panic.as_bytes())
        }) {
            signals.panic_macros += 1;
        }
    } else if kind == ownership.call_kind
        && let Some(callee) = node.child_by_field_name("function").map(|callee| {
            if callee.kind() == ownership.generic_callee_kind {
                callee.child_by_field_name("function").unwrap_or(callee)
            } else {
                callee
            }
        })
        && callee.kind() == ownership.method_kind
        && let Some(method) = callee.child_by_field_name("field")
    {
        let mut cursor = node.walk();
        let arguments = node.child_by_field_name("arguments").map_or(0, |list| {
            list.named_children(&mut cursor)
                .filter(|argument| !rules.tokens.comments.contains(&argument.kind()))
                .count()
        });
        let method = &syntax.source[method.byte_range()];
        let is_call = |shapes: &[CallShape]| {
            shapes
                .iter()
                .any(|shape| shape.arguments == arguments && shape.method.as_bytes() == method)
        };

        signals.clone_like_calls += u32::from(is_call(ownership.clone_like));
        signals.unwrap_like_calls += u32::from(is_call(ownership.unwrap_like));
    }
}

/// The statements directly inside a block, comments excluded.
fn statements<'tree>(block: Node<'tree>, rules: &LanguageRules) -> Vec<Node<'tree>> {
    let mut cursor = block.walk();
    let holder = match rules.statements.statement_list_kind {
        None => Some(block),
        Some(list_kind) => block
            .named_children(&mut cursor)
            .find(|child| child.kind() == list_kind),
    };

    let Some(holder) = holder else {
        return Vec::new();
    };

    let mut inner = holder.walk();

    holder
        .named_children(&mut inner)
        .filter(|child| !rules.statements.comment_kinds.contains(&child.kind()))
        .collect()
}

/// An `if` with no `else` whose block is one return, break, or continue.
fn is_guard_clause(statement: Node<'_>, signals: &SignalRules, rules: &LanguageRules) -> bool {
    if statement.kind() != rules.statements.if_kind
        || statement.child_by_field_name("alternative").is_some()
    {
        return false;
    }

    statement
        .child_by_field_name("consequence")
        .is_some_and(|block| {
            let inner = statements(block, rules);

            inner.len() == 1 && signals.guard_kinds.contains(&inner[0].kind())
        })
}

/// The single named, non-comment child of `node`, if it has exactly one.
fn only_child<'tree>(node: Node<'tree>, rules: &LanguageRules) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    let mut children = node
        .named_children(&mut cursor)
        .filter(|child| !rules.statements.comment_kinds.contains(&child.kind()));
    let first = children.next()?;

    children.next().is_none().then_some(first)
}

/// Keyed elements of a struct-like composite literal whose key is a bare
/// name and whose value is a name, a selector, or a predeclared constant.
fn field_mappings(literal: Node<'_>, signals: &SignalRules, rules: &LanguageRules) -> u32 {
    let struct_like = literal
        .child_by_field_name("type")
        .is_some_and(|ty| signals.composite_type_kinds.contains(&ty.kind()));
    let Some(body) = literal.child_by_field_name("body").filter(|_| struct_like) else {
        return 0;
    };

    let mut cursor = body.walk();
    let mut count = 0;

    for element in body
        .named_children(&mut cursor)
        .filter(|child| child.kind() == signals.keyed_element_kind)
    {
        let wrapped = |field: &str| {
            element
                .child_by_field_name(field)
                .filter(|wrapper| wrapper.kind() == signals.element_kind)
                .and_then(|wrapper| only_child(wrapper, rules))
        };
        let key_is_name =
            wrapped("key").is_some_and(|key| signals.mapping_key_kinds.contains(&key.kind()));
        let value_is_name = wrapped("value")
            .is_some_and(|value| signals.mapping_value_kinds.contains(&value.kind()));

        if key_is_name && value_is_name {
            count += 1;
        }
    }

    count
}

/// A call whose name reads like an assertion, or that is made on an
/// assertion package or a test handle. Everything is compared lowercase.
fn is_assertion_call(call: Node<'_>, signals: &SignalRules, source: &[u8]) -> bool {
    let Some(target) = (signals.call_target)(call, source) else {
        return false;
    };
    let name = target.name.to_lowercase();

    if name.is_empty() {
        return false;
    }

    if signals
        .assertion_prefixes
        .iter()
        .any(|prefix| name.starts_with(prefix))
    {
        return true;
    }

    let Some(receiver) = target.receiver else {
        return false;
    };
    let receiver = receiver.to_lowercase();

    signals.assertion_receivers.contains(&receiver.as_str())
        || (signals.testing_receivers.contains(&receiver.as_str())
            && signals.testing_methods.contains(&name.as_str()))
}
