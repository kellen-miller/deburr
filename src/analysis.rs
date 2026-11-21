//! Turns a parsed file into function units, findings, and a per-line code map.
//!
//! The engine is language independent: a [`LanguageRules`] table names the
//! node kinds that matter, plus a few shape functions per language.

use std::collections::HashMap;

use tree_sitter::Node;

use crate::identity::{self, FindingSketch, UnitSketch};
use crate::report::{
    Category, Detail, Finding, Function, HIGH_COMPLEXITY_THRESHOLD, Position, StructuralSignals,
};
use crate::syntax::{
    BranchShape, Enclosing, LanguageRules, SignalRules, Syntax, Token, UnitNaming, node_span,
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
    let rules = syntax.language.rules();
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

    for unit in &mut units {
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
    let mut bodies = Vec::with_capacity(units.len());

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
        bodies,
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

        candidates.extend(branch_candidates(node, owner, syntax, rules));

        let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
        pending.extend(children.into_iter().rev().map(|child| (child, inner)));
    }

    BodyScan {
        complexity,
        candidates,
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
        .filter(|child| child.kind() != rules.statements.comment_kind)
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
        .filter(|child| child.kind() != rules.statements.comment_kind);
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

/// The first and last real token's positions.
fn span_of(tokens: &[Token]) -> Option<(Position, Position)> {
    let mut real = tokens.iter().filter(|token| !token.is_terminator());
    let first = real.next()?;
    let last = real.next_back().unwrap_or(first);

    Some((first.start, last.end))
}

/// The findings that `node` starts, by the language's branch shape.
fn branch_candidates(
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
        } if kind == rules.statements.if_kind => {
            conditional_finding(node, else_kind, parenthesized_kind, owner, syntax, rules)
                .into_iter()
                .collect()
        }
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
fn boolean_return(statement: Node<'_>, rules: &LanguageRules) -> Option<bool> {
    if statement.kind() != rules.statements.return_kind {
        return None;
    }

    let results = only_child(statement, rules)?;
    let value = only_child(results, rules)?;

    boolean_literal(value, rules)
}

fn boolean_literal(node: Node<'_>, rules: &LanguageRules) -> Option<bool> {
    if node.kind() == rules.findings.true_kind {
        Some(true)
    } else if node.kind() == rules.findings.false_kind {
        Some(false)
    } else {
        None
    }
}

/// A TypeScript `if` and the branch after it, which is its `else` or, with
/// none, the next statement in the same block. Looks only at the `if`, never
/// at the code around it, so the finding fires at most once per `if`:
///
/// - `redundant-boolean-return` when each branch is a lone `return true` or
///   `return false` (bare or in a block), the literals differ, and the
///   condition, minus one pair of parentheses, is a comparison;
/// - otherwise `duplicate-branches` when the `then` branch is a non-empty
///   block with the same token text as the other branch. Equality needs a
///   braced `then` branch, so a bare statement branch is never a duplicate
///   and needs no statement terminator of its own.
///
/// The range runs to the end of the other branch when that is a following
/// statement. Neither finding carries more detail than its owner.
fn conditional_finding(
    node: Node<'_>,
    else_kind: &str,
    parenthesized_kind: &str,
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
            let in_block = node
                .parent()
                .is_some_and(|parent| parent.kind() == rules.statements.block_kind);
            let mut next = node.next_named_sibling();

            while next.is_some_and(|sibling| sibling.kind() == rules.statements.comment_kind) {
                next = next.and_then(|sibling| sibling.next_named_sibling());
            }

            next.filter(|_| in_block)?
        }
    };

    let literal = |branch: Node<'_>| {
        let statement = if branch.kind() == rules.statements.block_kind {
            let mut inner = statements(branch, rules).into_iter();

            inner.next().filter(|_| inner.next().is_none())?
        } else {
            branch
        };

        if statement.kind() != rules.statements.return_kind || statement.named_child_count() != 1 {
            return None;
        }

        boolean_literal(statement.named_child(0)?, rules)
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
