//! TypeScript analysis: exact measurements over the fixtures in `corpus/ts/`,
//! plus temporary trees for edits and exclusions.
//!
//! Each scenario is one directory, audited as its own root, so paths in the
//! report are relative to the scenario. Positions are 1-based with byte
//! columns; the same fixtures are compared against the Go version by the
//! parity harness, so the numbers here are the ones both versions report.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use deburr::audit::{AuditOptions, AuditOutcome, audit};
use deburr::report::{Category, FileStatus, Finding, Function, Report};

const FLOAT_TOLERANCE: f64 = 1e-9;
const REDUNDANT: &str = "redundant-boolean-return";
const DUPLICATE: &str = "duplicate-branches";

/// The exact measurements asserted for one function.
struct Expected {
    start: (u32, u32),
    end: (u32, u32),
    sloc: u32,
    cyclomatic: u32,
    max_nesting: u32,
    nested_branches: u32,
}

/// What one finding should say.
#[derive(Debug, PartialEq, Eq)]
struct ExpectedFinding<'a> {
    rule: &'a str,
    owner: &'a str,
    start: (u32, u32),
    end: (u32, u32),
}

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus/ts")
}

fn options() -> AuditOptions {
    AuditOptions {
        include_tests: true,
        include_testdata: false,
        include_vendor: false,
        include_generated: false,
        max_file_bytes: None,
        duplication: None,
    }
}

fn audit_root(root: &Path) -> AuditOutcome {
    audit(root, &options(), &mut io::sink())
}

fn scenario(name: &str) -> Report {
    audit_root(&corpus().join(name)).report
}

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent directories");
    }

    fs::write(path, contents).expect("write fixture");
}

fn function<'a>(report: &'a Report, name: &str) -> &'a Function {
    report
        .functions
        .iter()
        .find(|function| function.name == name)
        .unwrap_or_else(|| {
            let names: Vec<&str> = report.functions.iter().map(|f| f.name.as_str()).collect();

            panic!("function {name:?} not found among {names:?}")
        })
}

fn assert_function(report: &Report, name: &str, expected: &Expected) {
    let found = function(report, name);

    assert_eq!(
        (found.start.line, found.start.column),
        expected.start,
        "{name} start"
    );
    assert_eq!(
        (found.end.line, found.end.column),
        expected.end,
        "{name} end"
    );
    assert_eq!(found.sloc, expected.sloc, "{name} sloc");
    assert_eq!(found.cyclomatic, expected.cyclomatic, "{name} cyclomatic");
    assert_eq!(found.max_nesting, expected.max_nesting, "{name} nesting");
    assert_eq!(
        found.structural_signals.nested_branches, expected.nested_branches,
        "{name} nested branches"
    );

    let signals = &found.structural_signals;

    assert_eq!(
        (
            signals.flat_guards,
            signals.field_mappings,
            signals.assertion_like_calls
        ),
        (0, 0, 0),
        "{name}: TypeScript has no guard, mapping, or assertion signals"
    );

    let mass = f64::from(expected.cyclomatic) * f64::from(expected.sloc).sqrt();

    assert!((found.mass - mass).abs() < FLOAT_TOLERANCE, "{name} mass");
    assert_eq!(found.high_complexity, expected.cyclomatic > 10, "{name}");
}

fn owner(finding: &Finding) -> &str {
    finding
        .details
        .iter()
        .find(|detail| detail.key == "owner")
        .map_or("", |detail| detail.value.as_str())
}

fn assert_findings(report: &Report, path: &str, expected: &[ExpectedFinding<'_>]) {
    let found: Vec<ExpectedFinding<'_>> = report
        .findings
        .iter()
        .filter(|finding| finding.path == path)
        .map(|finding| ExpectedFinding {
            rule: finding.rule.as_str(),
            owner: owner(finding),
            start: (finding.start.line, finding.start.column),
            end: (finding.end.line, finding.end.column),
        })
        .collect();

    assert_eq!(found, expected, "findings in {path}");
}

/// The findings an audit of one source file reports, as `(rule, line)`.
fn findings_of(source: &str) -> Vec<(String, u32)> {
    let dir = tempfile::tempdir().expect("temp dir");

    write(dir.path(), "a.ts", source);

    audit_root(dir.path())
        .report
        .findings
        .iter()
        .map(|finding| (finding.rule.clone(), finding.start.line))
        .collect()
}

#[test]
fn metrics_and_findings_of_a_mixed_file() {
    let outcome = audit_root(&corpus().join("metrics-findings"));
    let report = &outcome.report;

    assert!(outcome.errors.is_empty(), "{:?}", outcome.errors);
    assert_eq!(report.functions.len(), 3);
    assert_function(
        report,
        "check",
        &Expected {
            start: (2, 8),
            end: (8, 1),
            // The nested arrow's four lines are its own.
            sloc: 3,
            // 1 + `&&` + the `if`.
            cyclomatic: 3,
            max_nesting: 1,
            nested_branches: 0,
        },
    );
    assert_function(
        report,
        "check.nested",
        &Expected {
            start: (3, 18),
            end: (6, 3),
            sloc: 4,
            cyclomatic: 2,
            max_nesting: 1,
            nested_branches: 0,
        },
    );
    assert_function(
        report,
        "Service.run",
        &Expected {
            start: (10, 3),
            end: (12, 3),
            sloc: 3,
            cyclomatic: 2,
            max_nesting: 1,
            nested_branches: 0,
        },
    );
    assert_eq!(report.files[0].code_lines, 12);
    assert_eq!(report.functions[0].id, "typescript/source.ts::check");

    // `check`'s `if`/`else` returns literals but its condition is a `&&`,
    // not a comparison, and its branches differ: no finding. `Service.run`'s
    // branches are identical.
    assert_findings(
        report,
        "source.ts",
        &[ExpectedFinding {
            rule: DUPLICATE,
            owner: "Service.run",
            start: (11, 5),
            end: (11, 71),
        }],
    );

    let finding = &report.findings[0];

    assert_eq!(finding.severity, "review");
    assert_eq!(
        finding.message,
        "conditional branches contain identical syntax; review consolidating them"
    );
    assert_eq!(
        finding
            .details
            .iter()
            .map(|detail| (detail.key.as_str(), detail.value.as_str()))
            .collect::<Vec<_>>(),
        [("owner", "Service.run"), ("candidate_only", "true")]
    );
}

#[test]
fn nested_controls_count_nesting_and_nested_branches() {
    let report = scenario("nested-controls");

    // 1 + `if` + `for` + `while` + one `case`; the `switch` itself adds
    // nesting but no decision.
    assert_function(
        &report,
        "nested",
        &Expected {
            start: (1, 1),
            end: (8, 1),
            sloc: 8,
            cyclomatic: 5,
            max_nesting: 3,
            // `for`, `while`, and `switch` are all entered inside something.
            nested_branches: 3,
        },
    );
}

#[test]
fn class_and_function_owners_prefix_names() {
    let report = scenario("class-owners");
    let names: Vec<&str> = report.functions.iter().map(|f| f.name.as_str()).collect();

    assert_eq!(
        names,
        [
            "A.run",
            "A.run.nested",
            "B.run",
            "factory",
            "factory.Inner.run",
            "run"
        ]
    );
    assert!(report.functions.iter().all(|f| !f.identity_ambiguous));
    // A method's class is not repeated on the arrow inside it.
    assert_eq!(
        function(&report, "A.run.nested").id,
        "typescript/owners.ts::A.run.nested"
    );
}

#[test]
fn names_come_from_the_function_or_what_holds_it() {
    let report = scenario("names");
    let named: Vec<&str> = report
        .functions
        .iter()
        .map(|function| function.name.as_str())
        .filter(|name| !name.starts_with("anon-"))
        .collect();

    assert_eq!(
        named,
        [
            // The node an arrow or function expression is assigned to.
            "arrow",
            "expression",
            // A function expression's own name wins over its holder's.
            "inner",
            "method",
            "Box.value",
            "Box.make",
            "Box.size",
            "Box.constructor",
            "Box.constructor.local",
            "outer",
            // A nameless function in a named one keeps the trailing dot.
            "outer.",
            "repeated",
            "repeated",
        ]
    );

    // `prop: () => 5` has nothing to name it: top level, so `anon-<digest>`.
    let anonymous: Vec<&Function> = report
        .functions
        .iter()
        .filter(|function| function.name.starts_with("anon-"))
        .collect();

    assert_eq!(anonymous.len(), 1);
    assert_eq!(
        anonymous[0].fingerprint.strip_prefix("sha256:"),
        anonymous[0].name.strip_prefix("anon-")
    );
    assert_eq!(
        anonymous[0].id,
        format!("typescript/names.ts::{}", anonymous[0].name)
    );
    assert!(!anonymous[0].identity_ambiguous);

    // A repeated name is the same function to the report's readers: both are
    // ambiguous, and the second gets a `#2` suffix in its ID.
    let repeated: Vec<&Function> = report
        .functions
        .iter()
        .filter(|function| function.name == "repeated")
        .collect();

    assert_eq!(repeated[0].id, "typescript/names.ts::repeated");
    assert_eq!(repeated[1].id, "typescript/names.ts::repeated#2");
    assert!(repeated.iter().all(|function| function.identity_ambiguous));
    assert_ne!(repeated[0].fingerprint, repeated[1].fingerprint);
}

#[test]
fn literals_keep_their_lines_and_generated_files_are_excluded() {
    let report = scenario("literals");

    // The comment line is not code; the regex and the two-line template are.
    assert_function(
        &report,
        "pattern",
        &Expected {
            start: (1, 24),
            end: (7, 1),
            sloc: 6,
            cyclomatic: 1,
            max_nesting: 0,
            nested_branches: 0,
        },
    );
    assert_eq!(report.coverage.excluded, 1);

    let generated = report
        .files
        .iter()
        .find(|file| file.path == "generated.ts")
        .expect("generated.ts is reported");

    assert_eq!(generated.category, Category::Generated);
    assert_eq!(generated.status, FileStatus::Excluded);
    assert_eq!(report.functions.len(), 1);
}

#[test]
fn whitespace_inside_a_template_literal_is_part_of_the_fingerprint() {
    let dir = tempfile::tempdir().expect("temp dir");
    let source =
        "export const pattern = () => {\nconst message = `first\n  second`;\nreturn message;\n};\n";

    write(dir.path(), "source.ts", source);

    let before = audit_root(dir.path()).report;

    write(
        dir.path(),
        "source.ts",
        &source.replace("  second", "second"),
    );

    let after = audit_root(dir.path()).report;

    assert_ne!(
        before.functions[0].fingerprint, after.functions[0].fingerprint,
        "template whitespace change lost from the fingerprint"
    );
}

#[test]
fn comments_and_layout_do_not_change_fingerprints_or_finding_ids() {
    let dir = tempfile::tempdir().expect("temp dir");
    let source =
        "export function check(n: number) { if (n > 1) { return true; } else { return false; } }\n";

    write(dir.path(), "source.ts", source);

    let before = audit_root(dir.path()).report;

    write(
        dir.path(),
        "source.ts",
        &format!("// shifted\n\n{source}\n// trailing\n"),
    );

    let after = audit_root(dir.path()).report;

    assert_eq!(before.findings.len(), 1);
    assert_eq!(after.findings.len(), 1);
    assert_eq!(before.findings[0].id, after.findings[0].id);
    assert_eq!(
        before.findings[0].fingerprint,
        after.findings[0].fingerprint
    );
    assert_eq!(
        before.functions[0].fingerprint,
        after.functions[0].fingerprint
    );
    assert_ne!(before.findings[0].start.line, after.findings[0].start.line);
}

#[test]
fn dialects_categories_and_declarations_without_bodies() {
    let report = scenario("syntax-coverage");
    let coverage = &report.coverage;

    // `broken.ts` fails to parse; `testdata/` is excluded; the rest analyze.
    assert_eq!(coverage.parse_errors, 1);
    assert_eq!(coverage.analyzed, 7);
    assert_eq!(coverage.excluded, 1);
    assert_eq!(report.metrics.test.functions, 3);
    assert_eq!(report.metrics.production.functions, 3);

    // `types.d.ts` only declares a function; there is no body to measure.
    assert!(report.functions.iter().all(|f| f.path != "types.d.ts"));

    let names: Vec<(&str, &str)> = report
        .functions
        .iter()
        .map(|f| (f.path.as_str(), f.name.as_str()))
        .collect();

    assert!(names.contains(&("component.tsx", "View")), "{names:?}");
    assert!(names.contains(&("module.mts", "value")), "{names:?}");
    assert!(names.contains(&("module.cts", "value")), "{names:?}");

    let broken = report
        .files
        .iter()
        .find(|file| file.path == "broken.ts")
        .expect("broken.ts is reported");

    assert_eq!(broken.status, FileStatus::ParseError);
    assert!(
        broken.error.starts_with("syntax error at 1:"),
        "{:?}",
        broken.error
    );
}

#[test]
fn excluding_tests_leaves_only_production_functions() {
    let mut without_tests = options();

    without_tests.include_tests = false;

    let report = audit(
        &corpus().join("syntax-coverage"),
        &without_tests,
        &mut io::sink(),
    )
    .report;

    assert_eq!(report.metrics.test.functions, 0);
    // The three test files and `testdata/`.
    assert_eq!(report.coverage.excluded, 4);
}

#[test]
fn excluded_files_keep_line_counts_and_report_no_functions() {
    let dir = tempfile::tempdir().expect("temp dir");

    write(
        dir.path(),
        "valid.test.ts",
        "// comment\nfunction value() {\n  return 1;\n}\n",
    );
    write(dir.path(), "invalid.test.ts", "function broken( {\n");

    let mut without_tests = options();

    without_tests.include_tests = false;

    let report = audit(dir.path(), &without_tests, &mut io::sink()).report;

    assert!(report.functions.is_empty());
    assert_eq!(report.coverage.excluded, 2);
    assert_eq!(report.coverage.parse_errors, 0);

    let valid = report
        .files
        .iter()
        .find(|file| file.path == "valid.test.ts")
        .expect("valid.test.ts is reported");

    assert_eq!(valid.status, FileStatus::Excluded);
    assert_eq!(valid.code_lines, 3);
}

#[test]
fn node_modules_is_skipped_without_parsing() {
    let dir = tempfile::tempdir().expect("temp dir");

    write(
        dir.path(),
        "node_modules/pkg/index.ts",
        "invalid syntax {{\n",
    );
    write(dir.path(), "ok.ts", "export const ok = () => 1;\n");

    let outcome = audit_root(dir.path());

    assert!(outcome.errors.is_empty(), "{:?}", outcome.errors);
    assert_eq!(outcome.report.coverage.excluded_directories, 1);
    assert_eq!(outcome.report.coverage.parse_errors, 0);
    assert_eq!(outcome.report.functions.len(), 1);
}

#[test]
fn boolean_returns_ignore_comments_in_every_branch_shape() {
    let report = scenario("boolean-guards");

    for (path, expected_owner) in [
        ("following.ts", "following"),
        ("following-comments.ts", "followingComments"),
        ("else-comments.ts", "elseComments"),
    ] {
        let findings: Vec<&Finding> = report.findings.iter().filter(|f| f.path == path).collect();

        assert_eq!(findings.len(), 1, "{path}");
        assert_eq!(findings[0].rule, REDUNDANT, "{path}");
        assert_eq!(owner(findings[0]), expected_owner, "{path}");
        assert_eq!(
            findings[0].message,
            "boolean branches return opposite literals; review replacing them with the condition or its negation"
        );
    }
}

#[test]
fn redundant_boolean_returns_need_a_comparison_and_opposite_literals() {
    let report = scenario("branches");

    assert_findings(
        &report,
        "booleans.ts",
        &[
            // A bare `if` and the `return` after it.
            ExpectedFinding {
                rule: REDUNDANT,
                owner: "opposite",
                start: (2, 3),
                end: (3, 15),
            },
            // Both branches written as blocks, `!==`, no semicolons.
            ExpectedFinding {
                rule: REDUNDANT,
                owner: "negated",
                start: (7, 3),
                end: (7, 52),
            },
            // No semicolons: the statements end at the line break.
            ExpectedFinding {
                rule: REDUNDANT,
                owner: "automatic",
                start: (26, 3),
                end: (27, 14),
            },
            // An arrow is a function like any other.
            ExpectedFinding {
                rule: REDUNDANT,
                owner: "arrow",
                start: (31, 3),
                end: (32, 14),
            },
            // Not reported: `((a < b))` is parenthesized twice, `if (a)` is
            // no comparison, and `sameLiteral` returns `true` twice.
        ],
    );
}

#[test]
fn duplicate_branches_need_a_braced_branch_that_matches_the_other() {
    let report = scenario("branches");

    assert_findings(
        &report,
        "duplicates.ts",
        &[
            // `{ log(1); }` against `{ log(1); }`.
            ExpectedFinding {
                rule: DUPLICATE,
                owner: "blocks",
                start: (8, 3),
                end: (8, 40),
            },
            // No `else`, so the next statement is the other branch; a block
            // statement that matches ends the range at its closing brace.
            ExpectedFinding {
                rule: DUPLICATE,
                owner: "sibling",
                start: (16, 3),
                end: (17, 13),
            },
            // Not reported: bare branches (`bare`), different, empty, and
            // comment-only branches, an `else if`, and a following statement
            // that is not a block.
        ],
    );
}

#[test]
fn bare_statement_branches_need_no_terminator_because_equality_needs_braces() {
    // A bare branch has no statement terminator of its own, so it tokenizes
    // differently from the same statement in a block. That never matters:
    // `duplicate-branches` needs a braced `then` branch, and a braced branch
    // never equals a bare one.
    assert!(findings_of("function f(x: boolean) { if (x) g(); else g(); }\n").is_empty());
    assert!(findings_of("function f(x: boolean) { if (x) g()\n else g() }\n").is_empty());
    assert!(findings_of("function f(x: boolean) { if (x) { g(); } else g(); }\n").is_empty());
    assert!(findings_of("function f(x: boolean) { if (x) g(); else { g(); } }\n").is_empty());

    // Inside a braced branch statements are terminated, so an explicit `;`
    // and a line break read alike and the branches match.
    for source in [
        "function f(x: boolean) { if (x) { g(); } else { g() } }\n",
        "function f(x: boolean) { if (x) { g()\n } else { g(); } }\n",
        "function f(x: boolean) { if (x) { g(); h() } else { g()\n h(); } }\n",
    ] {
        assert_eq!(findings_of(source), [(DUPLICATE.to_owned(), 1)], "{source}");
    }

    // And a different statement still differs.
    assert!(findings_of("function f(x: boolean) { if (x) { g(); } else { h(); } }\n").is_empty());
}

#[test]
fn a_following_statement_is_the_other_branch_only_inside_a_block() {
    // The same shape as the `sibling` fixture, but in a block statement.
    assert_eq!(
        findings_of("function f(x: boolean) {\n  if (x) { g(); }\n  { g(); }\n}\n"),
        [(DUPLICATE.to_owned(), 2)]
    );

    // A `program`-level `if` is outside any function, so nothing reports it.
    assert!(findings_of("declare const x: boolean;\nif (x) { g(); }\n{ g(); }\n").is_empty());
}

#[test]
fn a_statement_break_changes_the_fingerprint_a_semicolon_does_not() {
    let dir = tempfile::tempdir().expect("temp dir");

    write(
        dir.path(),
        "split.ts",
        "export function f(g: () => number) {\n  return\n  g()\n}\n",
    );
    write(
        dir.path(),
        "joined.ts",
        "export function f(g: () => number) {\n  return g()\n}\n",
    );
    write(
        dir.path(),
        "explicit.ts",
        "export function f(a: () => void, b: () => void) {\n  a();\n  b();\n}\n",
    );
    write(
        dir.path(),
        "automatic.ts",
        "export function f(a: () => void, b: () => void) {\n  a()\n  b()\n}\n",
    );

    let report = audit_root(dir.path()).report;
    let fingerprint = |path: &str| {
        report
            .functions
            .iter()
            .find(|function| function.path == path)
            .map(|function| function.fingerprint.clone())
            .unwrap_or_default()
    };

    // `return` then `g()` is two statements; `return g()` is one.
    assert_ne!(fingerprint("split.ts"), fingerprint("joined.ts"));
    // An explicit `;` and a line break end a statement alike.
    assert_eq!(fingerprint("explicit.ts"), fingerprint("automatic.ts"));
    assert!(!fingerprint("explicit.ts").is_empty());
}

#[test]
fn functions_inside_template_substitutions_are_measured() {
    let dir = tempfile::tempdir().expect("temp dir");

    write(
        dir.path(),
        "view.ts",
        "export function render(items: number[]) {\n  return `<ul>${items.map((item) => {\n    if (item > 1) { return 'big'; }\n    return 'small';\n  }).join('')}</ul>`;\n}\n",
    );

    let report = audit_root(dir.path()).report;

    assert_eq!(report.functions.len(), 2);
    assert_function(
        &report,
        "render.",
        &Expected {
            start: (2, 27),
            end: (5, 3),
            // The arrow owns every line it spans, the first one included.
            sloc: 4,
            cyclomatic: 2,
            max_nesting: 1,
            nested_branches: 0,
        },
    );
    assert_function(
        &report,
        "render",
        &Expected {
            start: (1, 8),
            end: (6, 1),
            sloc: 2,
            cyclomatic: 1,
            max_nesting: 0,
            nested_branches: 0,
        },
    );
}

#[test]
fn a_semicolon_on_its_own_line_is_code() {
    let dir = tempfile::tempdir().expect("temp dir");

    write(dir.path(), "a.ts", "export function f() {\n  g()\n  ;\n}\n");

    let report = audit_root(dir.path()).report;

    assert_eq!(report.files[0].code_lines, 4);
    assert_eq!(report.functions[0].sloc, 4);
}

#[test]
fn two_audits_of_one_tree_are_identical() {
    let first = serde_json::to_string(&audit_root(&corpus()).report).expect("encode");
    let second = serde_json::to_string(&audit_root(&corpus()).report).expect("encode");

    assert_eq!(first, second);
}

#[test]
fn function_ids_are_unique_within_a_report() {
    let report = audit_root(&corpus()).report;
    let mut ids: Vec<&str> = report.functions.iter().map(|f| f.id.as_str()).collect();
    let total = ids.len();

    ids.sort_unstable();
    ids.dedup();

    assert_eq!(ids.len(), total);
}
