//! Rust findings: `redundant-boolean-return` and `duplicate-branches`.
//!
//! Each case is one `src/lib.rs` audited as its own tree. Positions are
//! 1-based with byte columns.

use std::fs;
use std::io;

use deburr::audit::{AuditOptions, audit};
use deburr::report::{Finding, Position, Report};

const REDUNDANT: &str = "redundant-boolean-return";
const DUPLICATE: &str = "duplicate-branches";

/// A source and the rules it must be flagged with, in report order.
struct Case {
    label: &'static str,
    source: &'static str,
    rules: &'static [&'static str],
}

fn audit_source(source: &str) -> Report {
    let root = tempfile::tempdir().expect("create temp dir");

    fs::create_dir_all(root.path().join("src")).expect("create src");
    fs::write(root.path().join("src/lib.rs"), source).expect("write fixture");

    let options = AuditOptions {
        include_tests: true,
        include_testdata: false,
        include_vendor: false,
        include_generated: false,
        max_file_bytes: None,
        duplication: None,
    };
    let outcome = audit(root.path(), &options, &mut io::sink());

    assert!(outcome.errors.is_empty(), "{:?}", outcome.errors);
    assert_eq!(outcome.report.coverage.parse_errors, 0);

    outcome.report
}

fn assert_cases(cases: &[Case]) {
    for case in cases {
        let report = audit_source(case.source);
        let rules: Vec<&str> = report
            .findings
            .iter()
            .map(|finding| finding.rule.as_str())
            .collect();

        assert_eq!(rules, case.rules, "{}", case.label);
    }
}

fn detail<'a>(finding: &'a Finding, key: &str) -> &'a str {
    finding
        .details
        .iter()
        .find(|detail| detail.key == key)
        .map(|detail| detail.value.as_str())
        .unwrap_or_else(|| panic!("finding has no {key:?} detail: {:?}", finding.details))
}

const fn at(line: u32, column: u32) -> Position {
    Position { line, column }
}

#[test]
fn redundant_boolean_return_needs_a_comparison_and_opposite_literals() {
    assert_cases(&[
        Case {
            label: "tail literals",
            source: "fn f(a: u32, b: u32) -> bool {\n    if a > b { true } else { false }\n}\n",
            rules: &[REDUNDANT],
        },
        Case {
            label: "returns with semicolons",
            source: "fn f(a: u32, b: u32) -> bool {\n    if a == b {\n        return true;\n    } else {\n        return false;\n    }\n}\n",
            rules: &[REDUNDANT],
        },
        Case {
            label: "returns without semicolons",
            source: "fn f(a: u32, b: u32) -> bool {\n    if a != b {\n        return false\n    } else {\n        return true\n    }\n}\n",
            rules: &[REDUNDANT],
        },
        Case {
            label: "a tail literal against a return",
            source: "fn f(a: u32, b: u32) -> bool {\n    if a <= b {\n        return true;\n    } else {\n        false\n    }\n}\n",
            rules: &[REDUNDANT],
        },
        Case {
            label: "a parenthesized condition",
            source: "fn f(a: u32, b: u32) -> bool {\n    if (a >= b) { true } else { false }\n}\n",
            rules: &[REDUNDANT],
        },
        Case {
            label: "a let initializer",
            source: "fn f(a: u32, b: u32) -> bool {\n    let ok = if a < b { true } else { false };\n    ok\n}\n",
            rules: &[REDUNDANT],
        },
        Case {
            label: "a closure body",
            source: "fn f() -> impl Fn(u32) -> bool {\n    |a| if a == 1 { true } else { false }\n}\n",
            rules: &[REDUNDANT],
        },
        Case {
            label: "the next statement is a tail literal",
            source: "fn f(a: u32) -> bool {\n    if a > 1 {\n        return true;\n    }\n    false\n}\n",
            rules: &[REDUNDANT],
        },
        Case {
            label: "the next statement is a return",
            source: "fn f(a: u32) -> bool {\n    if a > 1 {\n        return false;\n    }\n    // fallback\n    /* kept */\n    return true;\n}\n",
            rules: &[REDUNDANT],
        },
        Case {
            label: "the condition is not a comparison",
            source: "fn f(flag: bool) -> bool {\n    if flag { true } else { false }\n}\n",
            rules: &[],
        },
        Case {
            label: "a logical condition is not a comparison",
            source: "fn f(a: u32, b: u32) -> bool {\n    if a > 1 && b > 1 { true } else { false }\n}\n",
            rules: &[],
        },
        Case {
            label: "if let is not a comparison",
            source: "fn f(a: Option<u32>) -> bool {\n    if let Some(_) = a { true } else { false }\n}\n",
            rules: &[],
        },
        Case {
            label: "matching literals are duplicate branches instead",
            source: "fn f(a: u32, b: u32) -> bool {\n    if a > b { true } else { true }\n}\n",
            rules: &[DUPLICATE],
        },
        Case {
            label: "a branch holds more than the literal",
            source: "fn f(a: u32, b: u32) -> bool {\n    if a > b {\n        g();\n        true\n    } else {\n        false\n    }\n}\nfn g() {}\n",
            rules: &[],
        },
        Case {
            label: "a branch is not a boolean",
            source: "fn f(a: u32, b: u32) -> u32 {\n    if a > b { 1 } else { 0 }\n}\n",
            rules: &[],
        },
        Case {
            label: "a literal followed by a semicolon is the unit value",
            source: "fn f(a: u32, b: u32) {\n    if a > b { true; } else { false; }\n}\n",
            rules: &[],
        },
        Case {
            label: "else if is not the other literal",
            source: "fn f(a: u32, b: u32, flag: bool) -> bool {\n    if a > b { true } else if flag { false } else { true }\n}\n",
            rules: &[],
        },
        Case {
            label: "the next statement is not adjacent",
            source: "fn f(a: u32) -> bool {\n    if a > 1 {\n        return true;\n    }\n    let b = a;\n    b > 3\n}\n",
            rules: &[],
        },
        Case {
            label: "the if is not in a block",
            source: "fn f(a: u32) -> bool {\n    let g = |x: u32| {\n        if x > 1 { return true; }\n        let y = x;\n        y > 5\n    };\n    g(a)\n}\n",
            rules: &[],
        },
    ]);
}

#[test]
fn redundant_boolean_return_reports_the_whole_condition_and_both_branches() {
    let report = audit_source(
        "fn f(x: u32) -> bool {\n    if x > 1 {\n        true\n    } else {\n        false\n    }\n}\n",
    );
    let [finding] = report.findings.as_slice() else {
        panic!("expected one finding, got {:?}", report.findings);
    };

    assert_eq!(finding.rule, REDUNDANT);
    assert_eq!(finding.path, "src/lib.rs");
    assert_eq!(finding.severity, "review");
    assert_eq!(
        finding.message,
        "boolean branches return opposite literals; review replacing them with the condition or its negation"
    );
    assert_eq!((finding.start, finding.end), (at(2, 5), at(6, 5)));
    assert_eq!(detail(finding, "owner"), "f");
    assert_eq!(detail(finding, "candidate_only"), "true");
    assert!(!finding.identity_ambiguous);
}

#[test]
fn redundant_boolean_return_in_sibling_form_ends_at_the_other_branch() {
    let report = audit_source(
        "fn f(x: u32) -> bool {\n    if x > 1 {\n        return true;\n    }\n    false\n}\n",
    );
    let [finding] = report.findings.as_slice() else {
        panic!("expected one finding, got {:?}", report.findings);
    };

    assert_eq!(finding.rule, REDUNDANT);
    assert_eq!((finding.start, finding.end), (at(2, 5), at(5, 9)));
}

#[test]
fn duplicate_branches_need_an_else_block_with_the_same_tokens() {
    assert_cases(&[
        Case {
            label: "identical blocks",
            source: "fn f(a: bool) {\n    if a { g(); } else { g(); }\n}\nfn g() {}\n",
            rules: &[DUPLICATE],
        },
        Case {
            label: "layout and comments do not matter",
            source: "fn f(a: bool) {\n    if a {\n        g(1); // first\n    } else {\n        /* second */ g( 1 );\n    }\n}\nfn g(_: u32) {}\n",
            rules: &[DUPLICATE],
        },
        Case {
            label: "an if let condition",
            source: "fn f(a: Option<u32>) {\n    if let Some(_) = a { g(); } else { g(); }\n}\nfn g() {}\n",
            rules: &[DUPLICATE],
        },
        Case {
            label: "a duplicate behind an else if",
            source: "fn f(a: bool, b: bool) {\n    if a {\n        g();\n    } else if b {\n        h();\n    } else {\n        h();\n    }\n}\nfn g() {}\nfn h() {}\n",
            rules: &[DUPLICATE],
        },
        Case {
            label: "different statements",
            source: "fn f(a: bool) {\n    if a { g(); } else { h(); }\n}\nfn g() {}\nfn h() {}\n",
            rules: &[],
        },
        Case {
            label: "an else if is never a duplicate",
            source: "fn f(a: bool, b: bool) {\n    if a { g(); } else if b { g(); }\n}\nfn g() {}\n",
            rules: &[],
        },
        Case {
            label: "empty blocks",
            source: "fn f(a: bool) {\n    if a {} else {}\n}\n",
            rules: &[],
        },
        Case {
            label: "blocks that only hold comments",
            source: "fn f(a: bool) {\n    if a { /* one */ } else { /* two */ }\n}\n",
            rules: &[],
        },
        Case {
            label: "no else",
            source: "fn f(a: bool) {\n    if a { g(); }\n    g();\n}\nfn g() {}\n",
            rules: &[],
        },
        Case {
            label: "a following block is not an else",
            source: "fn f(a: bool) {\n    if a { g(); }\n    { g(); }\n}\nfn g() {}\n",
            rules: &[],
        },
    ]);
}

#[test]
fn duplicate_branches_report_the_whole_if() {
    let report = audit_source(
        "fn f(a: bool) {\n    if a {\n        g();\n    } else {\n        g();\n    }\n}\nfn g() {}\n",
    );
    let [finding] = report.findings.as_slice() else {
        panic!("expected one finding, got {:?}", report.findings);
    };

    assert_eq!(finding.rule, DUPLICATE);
    assert_eq!(
        finding.message,
        "conditional branches contain identical syntax; review consolidating them"
    );
    assert_eq!((finding.start, finding.end), (at(2, 5), at(6, 5)));
    assert_eq!(detail(finding, "owner"), "f");
    assert_eq!(detail(finding, "candidate_only"), "true");
}

#[test]
fn a_finding_belongs_to_the_innermost_function() {
    let source = "\
fn outer(a: u32) -> bool {
    let check = |x: u32| if x > a { true } else { false };
    fn inner(b: bool) {
        if b { go(); } else { go(); }
    }
    check(a)
}
fn go() {}
";
    let report = audit_source(source);
    let owned: Vec<(&str, &str)> = report
        .findings
        .iter()
        .map(|finding| (finding.rule.as_str(), detail(finding, "owner")))
        .collect();

    assert_eq!(
        owned,
        [
            (REDUNDANT, "outer::{closure#1}"),
            (DUPLICATE, "outer::inner")
        ]
    );
}

#[test]
fn comments_and_layout_do_not_change_fingerprints_or_finding_ids() {
    let plain = "\
fn f(a: u32, b: u32, flag: bool) -> bool {
    if flag { g(); } else { g(); }
    if a > b { return true; }
    false
}
fn g() {}
";
    let edited = "\
// Leading note.
fn f(a: u32, b: u32, flag: bool) -> bool {
    if flag {
        g(); // same
    } else {
        /* again */ g();
    }

    if a > b {
        return true;
    }
    // done
    false
}
fn g() {}
";
    let before = audit_source(plain);
    let after = audit_source(edited);

    assert_eq!(before.findings.len(), 2);
    assert_eq!(after.findings.len(), 2);

    for (old, new) in before.findings.iter().zip(&after.findings) {
        assert_eq!(old.rule, new.rule);
        assert_eq!(old.id, new.id, "{} id", old.rule);
        assert_eq!(old.fingerprint, new.fingerprint, "{} fingerprint", old.rule);
    }

    assert_ne!(before.findings[0].id, before.findings[1].id);
    assert_eq!(
        before.functions[0].fingerprint,
        after.functions[0].fingerprint
    );
}

#[test]
fn findings_in_inline_tests_are_reported_and_marked_by_their_function() {
    let source = "\
#[cfg(test)]
mod tests {
    #[test]
    fn same_branches() {
        let a = true;
        if a { drop(1); } else { drop(1); }
    }
}
";
    let report = audit_source(source);
    let [finding] = report.findings.as_slice() else {
        panic!("expected one finding, got {:?}", report.findings);
    };

    assert_eq!(finding.rule, DUPLICATE);
    assert_eq!(detail(finding, "owner"), "tests::same_branches");
}
