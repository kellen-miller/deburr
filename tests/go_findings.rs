//! Go structural signals and findings: exact results over the fixtures in
//! `corpus/go/`, plus identity behavior over sources written on the fly.
//!
//! Each scenario directory is audited as its own root. Positions are 1-based
//! with byte columns and an inclusive end; fixtures indent with tabs, so a
//! statement at depth one starts in column 2.

use std::io;
use std::path::{Path, PathBuf};

use deburr::audit::{AuditOptions, audit};
use deburr::report::{Detail, Finding, Function, Report};

const REDUNDANT: &str = "redundant-boolean-return";
const DUPLICATE: &str = "duplicate-branches";
const REDUNDANT_MESSAGE: &str = "boolean branches return opposite literals; review replacing them with the condition or its negation";
const DUPLICATE_MESSAGE: &str = "if branches have identical syntax; review whether the condition and branch effects can be simplified";

/// The exact facts asserted for one finding.
struct ExpectedFinding {
    path: &'static str,
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
    owner: &'static str,
    /// The collapsed condition text; empty for `duplicate-branches`.
    condition: &'static str,
    /// The literal the `if` returns; empty for `duplicate-branches`.
    then_return: &'static str,
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

fn scenario(name: &str) -> Report {
    let root: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/corpus/go")
        .join(name);

    audit(&root, &options(), &mut io::sink()).report
}

/// Audits one Go file written into a fresh directory as `sample.go`.
fn audit_source(source: &str) -> Report {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::write(dir.path().join("sample.go"), source).expect("write source");

    audit(dir.path(), &options(), &mut io::sink()).report
}

fn findings_of<'a>(report: &'a Report, rule: &str, path: &str) -> Vec<&'a Finding> {
    report
        .findings
        .iter()
        .filter(|finding| finding.rule == rule && finding.path == path)
        .collect()
}

fn function<'a>(report: &'a Report, path: &str, name: &str) -> &'a Function {
    report
        .functions
        .iter()
        .find(|function| function.path == path && function.name == name)
        .unwrap_or_else(|| panic!("function {name:?} in {path} not found"))
}

fn detail(key: &str, value: &str) -> Detail {
    Detail {
        key: key.to_owned(),
        value: value.to_owned(),
    }
}

fn assert_findings(report: &Report, rule: &str, expected: &[ExpectedFinding]) {
    let mut actual: Vec<&Finding> = report
        .findings
        .iter()
        .filter(|finding| finding.rule == rule)
        .collect();
    actual.sort_by_key(|finding| (finding.path.clone(), finding.start));

    assert_eq!(
        actual.len(),
        expected.len(),
        "{rule} findings: {:?}",
        actual
            .iter()
            .map(|finding| (&finding.path, finding.start.line))
            .collect::<Vec<_>>()
    );

    for (finding, want) in actual.into_iter().zip(expected) {
        let label = format!("{rule} in {} at {}", want.path, want.start_line);
        let else_return = if want.then_return == "true" {
            "false"
        } else {
            "true"
        };
        let details = if rule == REDUNDANT {
            vec![
                detail("owner", want.owner),
                detail("condition", want.condition),
                detail("then_return", want.then_return),
                detail("else_return", else_return),
                detail("candidate_only", "true"),
            ]
        } else {
            vec![
                detail("owner", want.owner),
                detail("candidate_only", "true"),
                detail("preserve_condition", "true"),
            ]
        };
        let message = if rule == REDUNDANT {
            REDUNDANT_MESSAGE
        } else {
            DUPLICATE_MESSAGE
        };

        assert_eq!(finding.path, want.path, "{label}: path");
        assert_eq!(
            (finding.start.line, finding.start.column),
            (want.start_line, want.start_column),
            "{label}: start"
        );
        assert_eq!(
            (finding.end.line, finding.end.column),
            (want.end_line, want.end_column),
            "{label}: end"
        );
        assert_eq!(finding.details, details, "{label}: details");
        assert_eq!(finding.message, message, "{label}: message");
        assert_eq!(finding.severity, "review", "{label}: severity");
        assert!(!finding.fingerprint.is_empty(), "{label}: fingerprint");
        assert!(
            finding.id.starts_with(&format!("{rule}-")) && finding.id.len() == rule.len() + 17,
            "{label}: id {}",
            finding.id
        );
    }
}

#[test]
fn redundant_boolean_reports_comparisons_returning_opposite_literals() {
    let report = scenario("redundant-boolean");

    assert_findings(
        &report,
        REDUNDANT,
        &[
            ExpectedFinding {
                path: "reported.go",
                start_line: 8,
                start_column: 2,
                end_line: 11,
                end_column: 13,
                owner: "Equal",
                condition: "a == b",
                then_return: "true",
            },
            // Whitespace runs in the condition collapse to one space; the
            // `false`/`true` order is the other way around.
            ExpectedFinding {
                path: "reported.go",
                start_line: 15,
                start_column: 2,
                end_line: 18,
                end_column: 12,
                owner: "Less",
                condition: "a+b*2 < 3",
                then_return: "false",
            },
            // A method with the result written `(bool)`.
            ExpectedFinding {
                path: "reported.go",
                start_line: 22,
                start_column: 2,
                end_line: 25,
                end_column: 13,
                owner: "(*Gate).Open",
                condition: "value >= g.limit",
                then_return: "true",
            },
            // Inside a loop body: any block is scanned, not just the function's.
            ExpectedFinding {
                path: "reported.go",
                start_line: 30,
                start_column: 3,
                end_line: 33,
                end_column: 14,
                owner: "InLoop",
                condition: "value != limit",
                then_return: "true",
            },
        ],
    );
}

#[test]
fn redundant_boolean_rejects_every_near_miss() {
    let report = scenario("redundant-boolean");

    // Parenthesized and non-comparison conditions, initializers, equal
    // literals, `else`, named or multiple results, non-literal returns, an
    // intervening statement, case and select clause bodies (not blocks), and
    // function literals all stay silent.
    assert!(
        findings_of(&report, REDUNDANT, "rejected.go").is_empty(),
        "{:?}",
        findings_of(&report, REDUNDANT, "rejected.go")
    );
    assert!(findings_of(&report, DUPLICATE, "rejected.go").is_empty());

    // Every rejected function was still measured.
    assert_eq!(
        report
            .functions
            .iter()
            .filter(|function| function.path == "rejected.go")
            .count(),
        15
    );
}

#[test]
fn duplicate_branches_compares_token_text_not_layout() {
    let report = scenario("duplicate-branches");

    assert_findings(
        &report,
        DUPLICATE,
        &[
            ExpectedFinding {
                path: "branches.go",
                start_line: 6,
                start_column: 2,
                end_line: 12,
                end_column: 2,
                owner: "Same",
                condition: "",
                then_return: "",
            },
            // Spacing and a comment differ; the tokens do not.
            ExpectedFinding {
                path: "branches.go",
                start_line: 18,
                start_column: 2,
                end_line: 22,
                end_column: 2,
                owner: "Reformatted",
                condition: "",
                then_return: "",
            },
            // Only the innermost `if` of an `else if` chain has a plain
            // `else` block, and that is the one reported.
            ExpectedFinding {
                path: "branches.go",
                start_line: 47,
                start_column: 9,
                end_line: 51,
                end_column: 2,
                owner: "ChainTail",
                condition: "",
                then_return: "",
            },
            ExpectedFinding {
                path: "branches.go",
                start_line: 63,
                start_column: 3,
                end_line: 67,
                end_column: 3,
                owner: "Closure.func1",
                condition: "",
                then_return: "",
            },
        ],
    );
    assert!(findings_of(&report, REDUNDANT, "branches.go").is_empty());
}

#[test]
fn duplicate_branches_rejects_differences_else_if_and_empty_blocks() {
    let report = scenario("duplicate-branches");
    let owners: Vec<&str> = findings_of(&report, DUPLICATE, "branches.go")
        .iter()
        .flat_map(|finding| finding.details.iter())
        .filter(|detail| detail.key == "owner")
        .map(|detail| detail.value.as_str())
        .collect();

    // `Different` differs, `ElseIf` has no `else` block, `Empty` has no
    // statements, and `Split` differs only by where a statement ends:
    // `return g()` is not `return` followed by `g()`.
    for rejected in ["Different", "ElseIf", "Empty", "Split"] {
        assert!(!owners.contains(&rejected), "{rejected} was reported");
    }
}

#[test]
fn flat_guards_count_direct_guard_clauses_only() {
    let report = scenario("signals");

    // Two guards, one with an initializer. Two statements, an `else`, `goto`,
    // and the `if` inside the loop do not count.
    assert_eq!(
        function(&report, "guards.go", "Guards")
            .structural_signals
            .flat_guards,
        2
    );
    // `break` and `continue` guard; `fallthrough` does not.
    assert_eq!(
        function(&report, "guards.go", "Loops")
            .structural_signals
            .flat_guards,
        2
    );
}

#[test]
fn field_mappings_count_name_keys_with_name_like_values() {
    let report = scenario("signals");
    let build = function(&report, "mappings.go", "Build");

    // Config{..}: Enabled: true, Name, Limit: c.Limit, Next: nil, Count:
    // iota are mappings and Raw: 5 is not (5); pkg.Config{Name} (1);
    // struct{..}{A: name} (1); the inner &Config{Name: name} (1). Slice,
    // map, generic, and elided-type literals are not counted.
    assert_eq!(build.structural_signals.field_mappings, 8);
    // The closure's literal belongs to the closure.
    assert_eq!(
        function(&report, "mappings.go", "Build.func1")
            .structural_signals
            .field_mappings,
        1
    );
}

#[test]
fn assertion_like_calls_count_in_test_files_only() {
    let report = scenario("signals");
    let things = function(&report, "checks_test.go", "TestThings");

    // assert.Equal, require.NoError, assertValid, expectOK, t.Errorf,
    // t.Fatal, testify.Foo, and the generic generic.Assert[int, string].
    // assert.Equal[int](..) is not a selector call and `Equal` is no
    // assertion prefix, so it stays out, as do t.Log, other.Errorf, and
    // TestHelper.
    assert_eq!(things.structural_signals.assertion_like_calls, 8);
    assert_eq!(
        function(&report, "checks_test.go", "TestThings.func1")
            .structural_signals
            .assertion_like_calls,
        1
    );
    // The same calls in a production file do not count.
    assert_eq!(
        function(&report, "checks.go", "Things")
            .structural_signals
            .assertion_like_calls,
        0
    );
}

const EQUAL_SOURCE: &str = "package a

func Equal(a, b int) bool {
	if a == b {
		return true
	}
	return false
}
";

#[test]
fn finding_identity_survives_layout_and_position_changes() {
    let before = audit_source(EQUAL_SOURCE);
    let after = audit_source(&format!(
        "// A new header comment.\n\n{}",
        EQUAL_SOURCE.replace("if a == b {", "if   a == b   { // trailing")
    ));

    let (old, new) = (&before.findings[0], &after.findings[0]);

    assert_eq!(old.rule, REDUNDANT);
    assert_ne!(old.start, new.start);
    assert_eq!(old.id, new.id);
    assert_eq!(old.fingerprint, new.fingerprint);
}

#[test]
fn finding_identity_follows_flagged_code_and_owner() {
    let before = audit_source(EQUAL_SOURCE);
    let condition = audit_source(&EQUAL_SOURCE.replace("a == b", "a != b"));
    let owner_edit = audit_source(&EQUAL_SOURCE.replace("\tif a == b {", "\ta++\n\tif a == b {"));

    let (old, changed, grown) = (
        &before.findings[0],
        &condition.findings[0],
        &owner_edit.findings[0],
    );

    // Editing the flagged code changes both.
    assert_ne!(old.id, changed.id);
    assert_ne!(old.fingerprint, changed.fingerprint);
    // Editing elsewhere in the owner keeps the ID but changes the
    // fingerprint, so a reviewer sees the context moved.
    assert_eq!(old.id, grown.id);
    assert_ne!(old.fingerprint, grown.fingerprint);
}

#[test]
fn repeated_owner_identities_make_findings_ambiguous_and_distinct() {
    let report = audit_source(
        "package a

func init() {
	if flag {
		run()
	} else {
		run()
	}
}

func init() {
	if flag {
		run()
	} else {
		run()
	}
}
",
    );

    assert_eq!(report.findings.len(), 2);
    assert!(
        report
            .findings
            .iter()
            .all(|finding| finding.identity_ambiguous)
    );
    assert_ne!(report.findings[0].id, report.findings[1].id);
    assert_ne!(report.findings[0].start, report.findings[1].start);
}

#[test]
fn several_findings_of_one_rule_in_one_function_get_distinct_ids() {
    let report = audit_source(
        "package a

func F(flag bool) {
	if flag {
		run()
	} else {
		run()
	}
	if flag {
		run()
	} else {
		run()
	}
}
",
    );

    assert_eq!(report.findings.len(), 2);
    // Same owner, rule, and content: only the occurrence tells them apart.
    assert_ne!(report.findings[0].id, report.findings[1].id);
    assert!(
        report
            .findings
            .iter()
            .all(|finding| !finding.identity_ambiguous)
    );
}

#[test]
fn findings_are_sorted_by_path_and_position() {
    let report = scenario("duplicate-branches");
    let positions: Vec<_> = report
        .findings
        .iter()
        .map(|finding| (&finding.path, finding.start))
        .collect();
    let mut sorted = positions.clone();
    sorted.sort();

    assert_eq!(positions, sorted);
}
