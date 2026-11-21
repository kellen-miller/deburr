//! Turns a parsed file into function units, findings, and a per-line code map.
//!
//! The engine is language independent: a [`FunctionRules`] table names the
//! node kinds that matter, plus a few shape functions per language.

use std::collections::HashMap;

use tree_sitter::Node;

use crate::identity::{self, FindingSketch, UnitSketch};
use crate::report::{
    Category, Detail, Finding, Function, HIGH_COMPLEXITY_THRESHOLD, Position, StructuralSignals,
};
use crate::syntax::{Language, Syntax, Token, go};

/// Rule ID of an `if` that returns opposite boolean literals.
pub const REDUNDANT_BOOLEAN_RETURN: &str = "redundant-boolean-return";

/// Rule ID of an `if` whose two branches have identical syntax.
pub const DUPLICATE_BRANCHES: &str = "duplicate-branches";

/// Everything measured in one file.
#[derive(Debug, Default)]
pub struct FileAnalysis {
    pub functions: Vec<Function>,
    pub findings: Vec<Finding>,
    /// Index `n - 1` is true when line `n` holds code.
    pub code_lines: Vec<bool>,
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

/// A language's function-unit rules: plain data plus a few shape functions.
#[derive(Debug)]
pub struct FunctionRules {
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
    /// The declared name of a function node, if it has one.
    pub declared_name: fn(Node<'_>, &[u8]) -> Option<String>,
    /// The package (or module) segment of function IDs.
    pub package_name: fn(Node<'_>, &[u8]) -> String,
    /// Statement shapes shared by signals and findings.
    pub statements: StatementRules,
    pub signals: SignalRules,
    pub findings: FindingRules,
}

/// Where statements live in the tree.
#[derive(Debug)]
pub struct StatementRules {
    pub comment_kind: &'static str,
    /// A braced body; its statements sit in one child of `statement_list_kind`.
    pub block_kind: &'static str,
    pub statement_list_kind: &'static str,
    pub if_kind: &'static str,
    pub return_kind: &'static str,
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
    /// Function kinds that may own a redundant-boolean-return finding.
    pub redundant_owner_kinds: &'static [&'static str],
    /// Whether a function declares exactly one unnamed `bool` result.
    pub returns_plain_bool: fn(Node<'_>, &[u8]) -> bool,
    pub true_kind: &'static str,
    pub false_kind: &'static str,
    pub comparison_operators: &'static [&'static str],
    pub redundant_boolean_message: &'static str,
    pub duplicate_branches_message: &'static str,
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

/// A finding found in a body, before its owner's identity is known.
struct Candidate {
    rule: &'static str,
    message: &'static str,
    start: Position,
    end: Position,
    /// [`identity::token_content`] of the flagged code.
    content: Vec<u8>,
    details: Vec<Detail>,
}

/// Analyzes one parsed file.
#[must_use]
pub fn analyze(path: &str, category: Category, syntax: &Syntax) -> FileAnalysis {
    let code_lines = syntax.code_lines();
    let rules = match syntax.language {
        Language::Go => &go::FUNCTIONS,
        // TypeScript function rules arrive with the TypeScript milestone.
        Language::TypeScript | Language::Tsx => {
            return FileAnalysis {
                code_lines,
                ..FileAnalysis::default()
            };
        }
    };

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
    let identities = identity::assign_unit_identities(&sketches);

    assign_sloc(&mut units, &code_lines);

    for unit in &mut units {
        if let Some(body) = unit.node.child_by_field_name("body") {
            let owner = OwnerContext {
                name: &unit.name,
                redundant_owner: rules
                    .findings
                    .redundant_owner_kinds
                    .contains(&unit.node.kind())
                    && (rules.findings.returns_plain_bool)(unit.node, &syntax.source),
                test_file: category == Category::Test,
            };
            let scan = scan_body(body, &owner, syntax, rules);

            unit.complexity = scan.complexity;
            unit.candidates = scan.candidates;
        }
    }

    let package = (rules.package_name)(syntax.tree.root_node(), &syntax.source);
    let mut findings = Vec::new();
    let mut functions = Vec::with_capacity(units.len());

    for (unit, identity) in units.into_iter().zip(identities) {
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

        functions.push(Function {
            id: identity::function_id(&package, path, &identity.name),
            fingerprint: unit.fingerprint,
            path: path.to_owned(),
            category,
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
        findings,
        code_lines,
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
fn discover_units<'tree>(syntax: &'tree Syntax, rules: &FunctionRules) -> Vec<Unit<'tree>> {
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

/// Names units: a declaration's own name, else `file.funcN` (N counts every
/// top-level unit) or `Parent.funcN` (N counts the parent's direct literals).
fn name_units(units: &mut [Unit<'_>], syntax: &Syntax, rules: &FunctionRules) {
    let mut top_level = 0_usize;
    let mut literals = vec![0_usize; units.len()];

    for index in 0..units.len() {
        let declared = (rules.declared_name)(units[index].node, &syntax.source);

        units[index].declared = declared.is_some();
        units[index].name = match (declared, units[index].parent) {
            (Some(name), _) => name,
            (None, None) => format!("file.func{}", top_level + 1),
            (None, Some(parent)) => {
                literals[parent] += 1;

                format!("{}.func{}", units[parent].name, literals[parent])
            }
        };

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
    /// The owner may have redundant-boolean-return findings.
    redundant_owner: bool,
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
    rules: &FunctionRules,
) -> BodyScan {
    let mut complexity = Complexity {
        cyclomatic: 1,
        ..Complexity::default()
    };
    let mut candidates = Vec::new();

    complexity.signals.flat_guards = statements(body, rules)
        .into_iter()
        .filter(|statement| is_guard_clause(*statement, rules))
        .count()
        .try_into()
        .unwrap_or(u32::MAX);

    let mut cursor = body.walk();
    let mut pending = vec![(body, 0_u32)];

    while let Some((node, depth)) = pending.pop() {
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

        let mut inner = depth;

        if rules.nesting_kinds.contains(&kind) {
            if depth > 0 {
                complexity.signals.nested_branches += 1;
            }

            inner += 1;
            complexity.max_nesting = complexity.max_nesting.max(inner);
        }

        if kind == rules.signals.composite_kind {
            complexity.signals.field_mappings += field_mappings(node, rules);
        }

        if owner.test_file
            && rules.signals.call_kinds.contains(&kind)
            && is_assertion_call(node, rules, &syntax.source)
        {
            complexity.signals.assertion_like_calls += 1;
        }

        if kind == rules.statements.if_kind {
            candidates.extend(duplicate_branches(node, owner, syntax, rules));
        }

        if kind == rules.statements.block_kind && owner.redundant_owner {
            candidates.extend(redundant_boolean_returns(node, owner, syntax, rules));
        }

        let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
        pending.extend(children.into_iter().rev().map(|child| (child, inner)));
    }

    BodyScan {
        complexity,
        candidates,
    }
}

/// The statements directly inside a block, comments excluded.
fn statements<'tree>(block: Node<'tree>, rules: &FunctionRules) -> Vec<Node<'tree>> {
    let mut cursor = block.walk();
    let list = block
        .named_children(&mut cursor)
        .find(|child| child.kind() == rules.statements.statement_list_kind);

    let Some(list) = list else {
        return Vec::new();
    };

    let mut inner = list.walk();

    list.named_children(&mut inner)
        .filter(|child| child.kind() != rules.statements.comment_kind)
        .collect()
}

/// An `if` with no `else` whose block is one return, break, or continue.
fn is_guard_clause(statement: Node<'_>, rules: &FunctionRules) -> bool {
    if statement.kind() != rules.statements.if_kind
        || statement.child_by_field_name("alternative").is_some()
    {
        return false;
    }

    statement
        .child_by_field_name("consequence")
        .is_some_and(|block| {
            let inner = statements(block, rules);

            inner.len() == 1 && rules.signals.guard_kinds.contains(&inner[0].kind())
        })
}

/// The single named, non-comment child of `node`, if it has exactly one.
fn only_child<'tree>(node: Node<'tree>, rules: &FunctionRules) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    let mut children = node
        .named_children(&mut cursor)
        .filter(|child| child.kind() != rules.statements.comment_kind);
    let first = children.next()?;

    children.next().is_none().then_some(first)
}

/// Keyed elements of a struct-like composite literal whose key is a bare
/// name and whose value is a name, a selector, or a predeclared constant.
fn field_mappings(literal: Node<'_>, rules: &FunctionRules) -> u32 {
    let signals = &rules.signals;
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
fn is_assertion_call(call: Node<'_>, rules: &FunctionRules, source: &[u8]) -> bool {
    let signals = &rules.signals;
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

/// The first and last real token's positions.
fn span_of(tokens: &[Token]) -> Option<(Position, Position)> {
    let mut real = tokens.iter().filter(|token| !token.is_terminator());
    let first = real.next()?;
    let last = real.next_back().unwrap_or(first);

    Some((first.start, last.end))
}

/// `duplicate-branches`: an `if` with a plain `else` block whose token text
/// equals the `then` block's. An `else if` is not a duplicate branch.
fn duplicate_branches(
    node: Node<'_>,
    owner: &OwnerContext<'_>,
    syntax: &Syntax,
    rules: &FunctionRules,
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
    rules: &FunctionRules,
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
    rules: &FunctionRules,
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

    let then_value = boolean_return(inner[0], rules)?;
    let following_value = boolean_return(following, rules)?;

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
fn boolean_return(statement: Node<'_>, rules: &FunctionRules) -> Option<bool> {
    if statement.kind() != rules.statements.return_kind {
        return None;
    }

    let results = only_child(statement, rules)?;
    let value = only_child(results, rules)?;

    if value.kind() == rules.findings.true_kind {
        Some(true)
    } else if value.kind() == rules.findings.false_kind {
        Some(false)
    } else {
        None
    }
}

fn detail(key: &str, value: &str) -> Detail {
    Detail {
        key: key.to_owned(),
        value: value.to_owned(),
    }
}
