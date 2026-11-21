//! Go function analysis: exact metrics over the fixtures in `corpus/go/`.
//!
//! Each scenario is one directory, audited as its own root, so paths in the
//! report are relative to the scenario. Positions are 1-based with byte
//! columns. The fixtures start with `//go:build ignore` so the repository's
//! Go tooling skips them; deburr does not evaluate build tags.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use deburr::audit::{AuditOptions, AuditOutcome, audit};
use deburr::report::{Category, FileStatus, Function, Report};

const FLOAT_TOLERANCE: f64 = 1e-9;

/// The exact measurements asserted for one function.
struct Expected {
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
    sloc: u32,
    cyclomatic: u32,
    max_nesting: u32,
    nested_branches: u32,
}

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus/go")
}

fn audit_root(root: &Path) -> AuditOutcome {
    let options = AuditOptions {
        include_tests: true,
        include_testdata: false,
        include_vendor: false,
        include_generated: false,
        max_file_bytes: None,
        duplication: None,
    };

    audit(root, &options, &mut io::sink())
}

fn scenario(name: &str) -> Report {
    audit_root(&corpus().join(name)).report
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
        (expected.start_line, expected.start_column),
        "{name} start"
    );
    assert_eq!(
        (found.end.line, found.end.column),
        (expected.end_line, expected.end_column),
        "{name} end"
    );
    assert_eq!(found.sloc, expected.sloc, "{name} sloc");
    assert_eq!(found.cyclomatic, expected.cyclomatic, "{name} cyclomatic");
    assert_eq!(found.max_nesting, expected.max_nesting, "{name} nesting");
    assert_eq!(
        found.structural_signals.nested_branches, expected.nested_branches,
        "{name} nested branches"
    );

    let mass = f64::from(expected.cyclomatic) * f64::from(expected.sloc).sqrt();

    assert!((found.mass - mass).abs() < FLOAT_TOLERANCE, "{name} mass");
    assert_eq!(found.high_complexity, expected.cyclomatic > 10, "{name}");
}

fn ids(report: &Report) -> Vec<&str> {
    report.functions.iter().map(|f| f.id.as_str()).collect()
}

#[test]
fn branches_short_circuit_and_default_switch() {
    let report = scenario("decisions");

    assert_eq!(report.functions.len(), 1);
    assert_function(
        &report,
        "Decisions",
        &Expected {
            start_line: 5,
            start_column: 1,
            end_line: 13,
            end_column: 1,
            sloc: 9,
            // 1 + if + `&&`; `default` adds nothing.
            cyclomatic: 3,
            max_nesting: 1,
            nested_branches: 0,
        },
    );
    assert_eq!(report.functions[0].id, "sample/decisions.go::Decisions");
    assert!(!report.functions[0].identity_ambiguous);
}

#[test]
fn closures_are_separate_units_and_bodiless_declarations_are_skipped() {
    let report = scenario("closures");

    // `func Declared()` has no body and is not a unit.
    assert_eq!(report.functions.len(), 2);
    assert_function(
        &report,
        "Outer",
        &Expected {
            start_line: 5,
            start_column: 1,
            end_line: 14,
            end_column: 1,
            // Four lines outside the closure: the closure's six are its own.
            sloc: 4,
            cyclomatic: 1,
            max_nesting: 0,
            nested_branches: 0,
        },
    );
    assert_function(
        &report,
        "Outer.func1",
        &Expected {
            start_line: 7,
            start_column: 11,
            end_line: 12,
            end_column: 2,
            sloc: 6,
            cyclomatic: 2,
            max_nesting: 1,
            nested_branches: 0,
        },
    );
}

#[test]
fn nested_closure_decisions_do_not_reach_the_parent() {
    let report = scenario("nested-metrics");

    assert_eq!(report.functions.len(), 3);
    assert_function(
        &report,
        "Outer",
        &Expected {
            start_line: 5,
            start_column: 1,
            end_line: 10,
            end_column: 1,
            sloc: 6,
            cyclomatic: 2,
            max_nesting: 1,
            nested_branches: 0,
        },
    );
    assert_function(
        &report,
        "Nested",
        &Expected {
            start_line: 12,
            start_column: 1,
            end_line: 23,
            end_column: 1,
            sloc: 6,
            cyclomatic: 3,
            max_nesting: 1,
            nested_branches: 0,
        },
    );
    assert_function(
        &report,
        "Nested.func1",
        &Expected {
            start_line: 14,
            start_column: 12,
            end_line: 19,
            end_column: 3,
            sloc: 6,
            cyclomatic: 2,
            max_nesting: 1,
            nested_branches: 0,
        },
    );

    let all = &report.metrics.all;

    assert_eq!(all.functions, 3);
    assert_eq!(all.cyclomatic, 7);
    assert_eq!(all.max_nesting, 1);
    assert_eq!(all.high_complexity_functions, 0);
    assert_eq!(all.high_complexity_mass_share, Some(0.0));
}

#[test]
fn every_nesting_construct_and_case_is_counted_once() {
    let report = scenario("nesting");

    // 1 + for + if + `&&` + tagless-switch case + else-if + select case
    // + two type-switch cases; no `default` counts.
    assert_function(
        &report,
        "Deep",
        &Expected {
            start_line: 5,
            start_column: 1,
            end_line: 28,
            end_column: 1,
            sloc: 24,
            cyclomatic: 9,
            max_nesting: 4,
            // The `if`, its switch, the `else if`, and the select; the
            // outer `for` and the type switch are at depth zero.
            nested_branches: 4,
        },
    );
}

#[test]
fn multiline_literal_lines_belong_to_the_function() {
    let report = scenario("multiline-literal");

    assert_eq!(function(&report, "Values").sloc, 7);
    assert_eq!(report.metrics.all.code_lines, 8);
}

#[test]
fn crlf_raw_string_lines_belong_to_the_function() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let mut source = String::from("package sample\r\n\r\nfunc Text() string {\r\n\ttext := `");

    for letter in 'a'..='t' {
        source.push(letter);
        source.push_str("\r\n");
    }

    source.push_str("`\r\n\treturn text\r\n}\r\n");
    std::fs::write(dir.path().join("crlf.go"), source).expect("write fixture");

    let report = audit_root(dir.path()).report;

    assert_eq!(function(&report, "Text").sloc, 24);
}

#[test]
fn line_directives_do_not_move_positions() {
    let report = scenario("line-directive");

    assert_function(
        &report,
        "Directive",
        &Expected {
            start_line: 6,
            start_column: 1,
            end_line: 11,
            end_column: 1,
            sloc: 6,
            cyclomatic: 2,
            max_nesting: 1,
            nested_branches: 0,
        },
    );
    assert_eq!(function(&report, "Directive").path, "directive.go");
}

#[test]
fn top_level_literals_are_numbered_among_all_top_level_units() {
    let report = scenario("top-level-literals");
    let names: Vec<&str> = report.functions.iter().map(|f| f.name.as_str()).collect();

    // `Second` is the third top-level unit, so the literal after it is the
    // fourth.
    assert_eq!(names, ["file.func1", "file.func2", "Second", "file.func4"]);

    // The first two literals have identical tokens and no parent, so they
    // cannot be told apart; the distinct literal and the declaration can.
    let ambiguous: Vec<bool> = report
        .functions
        .iter()
        .map(|f| f.identity_ambiguous)
        .collect();

    assert_eq!(ambiguous, [true, true, false, false]);
}

#[test]
fn receivers_name_methods() {
    let report = scenario("receivers");
    let names: Vec<&str> = report.functions.iter().map(|f| f.name.as_str()).collect();

    assert_eq!(
        names,
        [
            "T.Value",
            "(*T).Pointer",
            "(*G).Generic",
            "H.Multi",
            "receiver.Paren",
            "(*receiver).PointerParen",
            "receiver.Qualified",
        ]
    );
    assert_eq!(report.functions[2].id, "sample/receivers.go::(*G).Generic");
    assert!(report.functions.iter().all(|f| !f.identity_ambiguous));
}

#[test]
fn several_init_functions_get_numbered_ambiguous_identities() {
    let report = scenario("init-functions");
    let init_ids: Vec<&str> = report
        .functions
        .iter()
        .filter(|f| f.name == "init")
        .map(|f| f.id.as_str())
        .collect();

    assert_eq!(
        init_ids,
        [
            "sample/init.go::init",
            "sample/init.go::init#2",
            "sample/init.go::init#3"
        ]
    );
    assert!(
        report
            .functions
            .iter()
            .filter(|f| f.name == "init")
            .all(|f| f.identity_ambiguous)
    );

    // The closure inside the second `init` is named by its content under
    // that init's numbered identity, and inherits the ambiguity.
    let closure = function(&report, "init.func1");

    assert!(closure.id.starts_with("sample/init.go::init#2.anon-"));
    assert!(closure.identity_ambiguous);
}

#[test]
fn identities_ignore_inserted_lines() {
    let before = scenario("init-functions");
    let after = scenario("init-functions-shifted");

    assert_eq!(ids(&before), ids(&after));

    for (left, right) in before.functions.iter().zip(&after.functions) {
        assert_eq!(left.fingerprint, right.fingerprint, "{}", left.id);
        assert_ne!(left.start, right.start, "{} should have moved", left.id);
    }
}

#[test]
fn anonymous_identity_follows_content_not_order() {
    let original = scenario("anonymous");
    let swapped = scenario("anonymous-swapped");
    let by_fingerprint: BTreeMap<&str, &str> = original
        .functions
        .iter()
        .filter(|f| f.name != "Outer")
        .map(|f| (f.fingerprint.as_str(), f.id.as_str()))
        .collect();

    assert_eq!(by_fingerprint.len(), 2);

    for function in swapped.functions.iter().filter(|f| f.name != "Outer") {
        assert_eq!(
            by_fingerprint.get(function.fingerprint.as_str()),
            Some(&function.id.as_str()),
            "anonymous identity followed ordinal"
        );
        assert!(!function.identity_ambiguous);
    }
}

#[test]
fn identical_literals_under_one_parent_are_ambiguous() {
    let report = scenario("duplicate-literals");
    let closures: Vec<&Function> = report
        .functions
        .iter()
        .filter(|f| f.name != "Outer")
        .collect();

    assert_eq!(closures.len(), 3);
    assert_eq!(closures[1].id, format!("{}#2", closures[0].id));
    assert!(!closures[2].id.contains('#'));
    assert_ne!(closures[0].id, closures[2].id);
    assert!(closures[0].identity_ambiguous && closures[1].identity_ambiguous);
    assert!(!closures[2].identity_ambiguous);
    assert!(!function(&report, "Outer").identity_ambiguous);
}

#[test]
fn statement_boundaries_change_the_fingerprint() {
    let joined = scenario("terminator-joined");
    let split = scenario("terminator-split");

    // `return g()` and `return` newline `g()` have the same visible leaves.
    assert_ne!(
        function(&joined, "F").fingerprint,
        function(&split, "F").fingerprint
    );
    assert!(function(&joined, "F").fingerprint.starts_with("sha256:"));
    assert_eq!(
        function(&joined, "F").fingerprint,
        function(&scenario("terminator-joined"), "F").fingerprint
    );
}

#[test]
fn functions_carry_their_file_category() {
    let report = scenario("categories");

    assert_eq!(
        function(&report, "Production").category,
        Category::Production
    );
    assert_eq!(function(&report, "TestProduction").category, Category::Test);
    assert_eq!(report.metrics.production.functions, 1);
    assert_eq!(report.metrics.test.functions, 1);
    assert_eq!(report.metrics.all.functions, 2);
    assert_eq!(report.metrics.production.cyclomatic, 2);
    assert_eq!(report.metrics.test.cyclomatic, 1);
}

#[test]
fn files_that_fail_the_syntax_check_yield_no_functions() {
    let outcome = audit_root(&corpus().join("parse-errors"));
    let status = |path: &str| {
        outcome
            .report
            .files
            .iter()
            .find(|file| file.path == path)
            .map(|file| file.status)
    };

    assert_eq!(status("broken.go"), Some(FileStatus::ParseError));
    // tree-sitter-go accepts a file with no package clause; go/parser does
    // not, and neither do we.
    assert_eq!(status("no_package.go"), Some(FileStatus::ParseError));
    assert_eq!(status("good.go"), Some(FileStatus::Analyzed));
    assert_eq!(outcome.errors.len(), 2);
    assert_eq!(outcome.report.coverage.parse_errors, 2);
    assert_eq!(ids(&outcome.report), ["sample/good.go::Good"]);
}

/// Drops a trailing `#<digits>` occurrence suffix.
fn split_occurrence(id: &str) -> (&str, Option<u32>) {
    match id.rsplit_once('#') {
        Some((base, digits)) => match digits.parse() {
            Ok(number) => (base, Some(number)),
            Err(_) => (id, None),
        },
        None => (id, None),
    }
}

#[test]
fn ids_are_unique_and_suffixed_only_where_identities_repeat() {
    let report = audit_root(&corpus()).report;
    let mut groups: BTreeMap<&str, Vec<Option<u32>>> = BTreeMap::new();

    for function in &report.functions {
        let (base, occurrence) = split_occurrence(&function.id);

        groups.entry(base).or_default().push(occurrence);
    }

    assert!(report.functions.len() > 30, "corpus should be substantial");

    let mut unique: Vec<&str> = ids(&report);
    unique.sort_unstable();
    unique.dedup();

    assert_eq!(unique.len(), report.functions.len(), "IDs must be unique");

    for (base, occurrences) in &groups {
        if occurrences.len() == 1 {
            assert_eq!(occurrences[0], None, "{base} has a suffix but no repeat");

            continue;
        }

        let expected: Vec<Option<u32>> = std::iter::once(None)
            .chain((2..).take(occurrences.len() - 1).map(Some))
            .collect();

        assert_eq!(occurrences, &expected, "{base} suffixes");
    }
}

#[test]
fn identities_are_stable_across_audits() {
    let first = audit_root(&corpus()).report;
    let second = audit_root(&corpus()).report;

    assert_eq!(ids(&first), ids(&second));
    assert!(
        ids(&first).iter().any(|id| id.contains(".anon-")),
        "corpus should contain anonymous identities"
    );

    for (left, right) in first.functions.iter().zip(&second.functions) {
        assert_eq!(left.fingerprint, right.fingerprint);
        assert_eq!(left.identity_ambiguous, right.identity_ambiguous);
    }
}

#[test]
fn bucket_totals_match_the_functions() {
    let report = audit_root(&corpus()).report;
    let all = &report.metrics.all;
    let cyclomatic: u32 = report.functions.iter().map(|f| f.cyclomatic).sum();
    let mass: f64 = report.functions.iter().map(|f| f.mass).sum();
    let high = report
        .functions
        .iter()
        .filter(|f| f.high_complexity)
        .count();

    assert_eq!(all.functions as usize, report.functions.len());
    assert_eq!(all.cyclomatic, cyclomatic);
    assert!((all.mass - mass).abs() < FLOAT_TOLERANCE);
    assert_eq!(all.high_complexity_functions as usize, high);
    assert_eq!(all.high_complexity_functions, 0);
}
