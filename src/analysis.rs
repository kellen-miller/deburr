//! Turns a parsed file into function units, findings, and a per-line code map.
//!
//! The engine is language independent: a [`FunctionRules`] table names the
//! node kinds that matter, plus one naming function per language.

use tree_sitter::Node;

use crate::identity::{self, UnitSketch};
use crate::report::{
    Category, Finding, Function, HIGH_COMPLEXITY_THRESHOLD, Position, StructuralSignals,
};
use crate::syntax::{Language, Syntax, go};

/// Everything measured in one file.
#[derive(Debug, Default)]
pub struct FileAnalysis {
    pub functions: Vec<Function>,
    pub findings: Vec<Finding>,
    /// Index `n - 1` is true when line `n` holds code.
    pub code_lines: Vec<bool>,
}

/// A language's function-unit rules: plain data plus two naming functions.
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
}

/// What the walk over one function body measures.
#[derive(Debug, Clone, Copy)]
struct Complexity {
    cyclomatic: u32,
    max_nesting: u32,
    nested_branches: u32,
}

/// Analyzes one parsed file. Findings and the remaining structural signals
/// arrive with the findings milestone; the code-line map and function
/// metrics are real.
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
            unit.complexity = measure(body, rules);
        }
    }

    let package = (rules.package_name)(syntax.tree.root_node(), &syntax.source);
    let functions = units
        .into_iter()
        .zip(identities)
        .map(|(unit, identity)| {
            let mass = if unit.sloc == 0 {
                0.0
            } else {
                f64::from(unit.complexity.cyclomatic) * f64::from(unit.sloc).sqrt()
            };

            Function {
                id: identity::function_id(&package, path, &identity.name),
                fingerprint: unit.fingerprint,
                path: path.to_owned(),
                category,
                name: unit.name,
                structural_signals: StructuralSignals {
                    nested_branches: unit.complexity.nested_branches,
                    ..StructuralSignals::default()
                },
                start: unit.start,
                end: unit.end,
                sloc: unit.sloc,
                cyclomatic: unit.complexity.cyclomatic,
                max_nesting: unit.complexity.max_nesting,
                mass,
                high_complexity: unit.complexity.cyclomatic > HIGH_COMPLEXITY_THRESHOLD,
                identity_ambiguous: identity.ambiguous,
            }
        })
        .collect();

    FileAnalysis {
        functions,
        findings: Vec::new(),
        code_lines,
    }
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
            // Tokens that start inside the node. A statement terminator
            // that starts exactly at the node's end belongs to the
            // enclosing statement, so the strict comparison leaves it out.
            let first = syntax
                .tokens
                .partition_point(|token| token.bytes.start < node.start_byte());
            let last = syntax
                .tokens
                .partition_point(|token| token.bytes.start < node.end_byte());
            let tokens = &syntax.tokens[first..last];

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
                        max_nesting: 0,
                        nested_branches: 0,
                    },
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

/// Cyclomatic complexity, maximum nesting, and nested branches of one body.
/// Nested function units are skipped: their decisions belong to them.
fn measure(body: Node<'_>, rules: &FunctionRules) -> Complexity {
    let mut complexity = Complexity {
        cyclomatic: 1,
        max_nesting: 0,
        nested_branches: 0,
    };
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
                complexity.nested_branches += 1;
            }

            inner += 1;
            complexity.max_nesting = complexity.max_nesting.max(inner);
        }

        let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
        pending.extend(children.into_iter().map(|child| (child, inner)));
    }

    complexity
}
