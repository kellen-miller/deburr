//! The shared human summary: verdict, hints, ranking, review filtering,
//! advice, and number wording.

mod summary_fixture;

use deburr::render::summary::{FactState, WorkKind, count, percent, summarize};
use deburr::report::Category;
use serde_json::json;
use summary_fixture::{Region, base_report, clone, function, item};

#[test]
fn numbers_are_worded_with_separators_and_one_decimal() {
    assert_eq!(count(0), "0");
    assert_eq!(count(999), "999");
    assert_eq!(count(8219), "8,219");
    assert_eq!(count(1_000_000), "1,000,000");
    assert_eq!(percent(3.27), "3.3%");
    assert_eq!(percent(12.0), "12.0%");
    assert_eq!(percent(0.0), "0.0%");
}

#[test]
fn complexity_verdict_counts_functions_and_production_share() {
    let mut report = base_report();
    report.functions = (0..6)
        .map(|index| function("a.go", &format!("F{index}"), index * 100 + 1, 20, 12))
        .collect();
    report.metrics.production.high_complexity_mass_share = Some(0.076);
    let verdict = summarize(&report, "repo").verdict;

    assert_eq!(verdict.complexity.state, FactState::Attention);
    assert_eq!(
        verdict.complexity.headline,
        "6 functions are over the complexity limit"
    );
    assert_eq!(
        verdict.complexity.detail,
        "All six are in production code and hold 7.6% of its complexity. No test function is over."
    );

    let clear = summarize(&base_report(), "repo").verdict.complexity;
    assert_eq!(clear.state, FactState::Clear);
    assert_eq!(clear.headline, "No function is over the complexity limit");

    report.functions[0].category = Category::Test;
    let mixed = summarize(&report, "repo").verdict.complexity;
    assert!(
        mixed.detail.contains("five are in production code"),
        "{}",
        mixed.detail
    );
    assert!(mixed.detail.contains("one are in test code"));
}

#[test]
fn duplication_verdict_words_each_category_and_the_unmeasured_case() {
    let mut report = base_report();
    report.duplication.categories = serde_json::from_value(json!([
        { "category": "production", "files": 90, "code_lines": 8219,
          "duplicated_code_lines": 269, "percentage": 3.2730 },
        { "category": "test", "files": 40, "code_lines": 7695,
          "duplicated_code_lines": 926, "percentage": 12.0338 }
    ]))
    .expect("categories");
    let duplication = summarize(&report, "repo").verdict.duplication;

    assert_eq!(duplication.state, FactState::Attention);
    assert_eq!(
        duplication.headline,
        "3.3% of production code copied (269 of 8,219 lines)"
    );
    assert_eq!(
        duplication.detail,
        "12.0% of tests copied (926 of 7,695 lines)"
    );

    report.duplication.status = deburr::report::DuplicationStatus::NotRequested;
    let skipped = summarize(&report, "repo").verdict.duplication;
    assert_eq!(skipped.state, FactState::NotMeasured);
    assert_eq!(skipped.headline, "Duplication was not measured");
    assert!(
        skipped.detail.contains("--no-duplicates"),
        "{}",
        skipped.detail
    );

    report.duplication.status = deburr::report::DuplicationStatus::Error;
    report.duplication.error = "detector exploded".to_owned();
    let failed = summarize(&report, "repo").verdict.duplication;
    assert_eq!(failed.state, FactState::NotMeasured);
    assert_eq!(failed.detail, "detector exploded");
}

#[test]
fn findings_verdict_groups_by_rule_or_lists_the_rules_that_ran() {
    let none = summarize(&base_report(), "repo").verdict.findings;
    assert_eq!(none.state, FactState::Clear);
    assert_eq!(none.headline, "No findings");
    assert_eq!(
        none.detail,
        "Rules run: duplicate-branches, redundant-boolean-return."
    );

    let mut report = base_report();
    report.findings = (0..3)
        .map(|index| {
            serde_json::from_value(json!({
                "id": format!("f{index}"), "rule": if index < 2 { "duplicate-branches" } else { "redundant-boolean-return" },
                "path": "a.go", "severity": "review", "message": "m",
                "start": { "line": index + 1, "column": 1 },
                "end": { "line": index + 2, "column": 1 }
            }))
            .expect("finding")
        })
        .collect();
    let findings = summarize(&report, "repo").verdict.findings;

    assert_eq!(findings.state, FactState::Attention);
    assert_eq!(findings.headline, "3 findings");
    assert_eq!(
        findings.detail,
        "duplicate-branches ×2, redundant-boolean-return ×1"
    );
}

#[test]
fn group_advice_follows_category_and_locality() {
    let mut report = base_report();
    report.functions = vec![
        function("pkg/one.go", "A", 1, 20, 3),
        function("pkg/one.go", "B", 40, 20, 3),
        function("pkg/two.go", "C", 1, 20, 3),
        function("pkg/four.go", "E", 1, 20, 3),
        function("other/three.go", "D", 1, 20, 3),
        function("pkg/zzz.go", "G", 1, 20, 3),
        function("pkg/t_test.go", "T1", 1, 20, 3),
        function("pkg/u_test.go", "T2", 1, 20, 3),
    ];
    report.duplication.clones = vec![
        clone(
            "file",
            Category::Production,
            &[
                Region {
                    path: "pkg/one.go",
                    start: 2,
                    end: 12,
                    functions: &["pkg/pkg/one.go::A"],
                },
                Region {
                    path: "pkg/one.go",
                    start: 41,
                    end: 51,
                    functions: &["pkg/pkg/one.go::B"],
                },
            ],
        ),
        clone(
            "package",
            Category::Production,
            &[
                Region {
                    path: "pkg/two.go",
                    start: 2,
                    end: 12,
                    functions: &["pkg/pkg/two.go::C"],
                },
                Region {
                    path: "pkg/four.go",
                    start: 2,
                    end: 12,
                    functions: &["pkg/pkg/four.go::E"],
                },
            ],
        ),
        clone(
            "across",
            Category::Production,
            &[
                Region {
                    path: "other/three.go",
                    start: 2,
                    end: 12,
                    functions: &["pkg/other/three.go::D"],
                },
                Region {
                    path: "pkg/zzz.go",
                    start: 2,
                    end: 12,
                    functions: &["pkg/pkg/zzz.go::G"],
                },
            ],
        ),
        clone(
            "tests",
            Category::Test,
            &[
                Region {
                    path: "pkg/t_test.go",
                    start: 2,
                    end: 12,
                    functions: &["pkg/pkg/t_test.go::T1"],
                },
                Region {
                    path: "pkg/u_test.go",
                    start: 2,
                    end: 12,
                    functions: &["pkg/pkg/u_test.go::T2"],
                },
            ],
        ),
    ];
    let summary = summarize(&report, "repo");
    let advice_of = |needle: &str| {
        summary
            .worklist
            .iter()
            .find(|item| item.locations.iter().any(|place| place.path == needle))
            .and_then(|item| item.advice.clone())
            .unwrap_or_default()
    };

    assert_eq!(
        advice_of("pkg/one.go"),
        "Extract one shared function in this file."
    );
    assert_eq!(
        advice_of("pkg/two.go"),
        "Extract one shared function in this package."
    );
    assert_eq!(
        advice_of("other/three.go"),
        "Keep one copy in a shared package and call it from the others."
    );
    assert_eq!(
        advice_of("pkg/t_test.go"),
        "Use a table-driven test or a shared fixture."
    );
}

#[test]
fn worklist_ranks_findings_then_production_then_tests_by_size() {
    let mut report = base_report();
    let mut test_function = function("a_test.go", "TestBig", 1, 90, 14);
    test_function.category = Category::Test;
    report.functions = vec![
        function("big.go", "Big", 1, 30, 14),
        function("small.go", "Small", 1, 12, 14),
        test_function,
        function("c.go", "C1", 1, 20, 3),
        function("d.go", "D1", 1, 20, 3),
    ];
    report.findings = vec![
        serde_json::from_value(json!({
            "id": "f1", "rule": "duplicate-branches", "path": "z.go", "severity": "review",
            "message": "same branches",
            "start": { "line": 3, "column": 1 }, "end": { "line": 6, "column": 1 }
        }))
        .expect("finding"),
    ];
    report.duplication.clones = vec![
        clone(
            "prod",
            Category::Production,
            &[
                Region {
                    path: "c.go",
                    start: 2,
                    end: 21,
                    functions: &["pkg/c.go::C1"],
                },
                Region {
                    path: "d.go",
                    start: 2,
                    end: 21,
                    functions: &["pkg/d.go::D1"],
                },
            ],
        ),
        clone(
            "test",
            Category::Test,
            &[
                Region {
                    path: "a_test.go",
                    start: 100,
                    end: 199,
                    functions: &[],
                },
                Region {
                    path: "b_test.go",
                    start: 100,
                    end: 199,
                    functions: &[],
                },
            ],
        ),
    ];
    let summary = summarize(&report, "repo");
    let order: Vec<(WorkKind, Category, u32)> = summary
        .worklist
        .iter()
        .map(|item| (item.kind, item.category, item.size))
        .collect();

    assert_eq!(
        order,
        [
            (WorkKind::Finding, Category::Production, 0),
            (WorkKind::Copied, Category::Production, 40),
            (WorkKind::Complex, Category::Production, 30),
            (WorkKind::Complex, Category::Production, 12),
            (WorkKind::Copied, Category::Test, 200),
            (WorkKind::Complex, Category::Test, 90),
        ]
    );
    assert_eq!(
        summary.worklist[0].advice.as_deref(),
        Some("Merge the identical branches.")
    );
}

#[test]
fn settled_items_are_hidden_and_counted_and_deferred_ones_stay_marked() {
    let mut report = base_report();
    report.functions = vec![
        function("a.go", "Retained", 1, 20, 14),
        function("b.go", "Deferred", 1, 20, 14),
        function("c.go", "Open", 1, 20, 14),
    ];
    report.findings = vec![
        serde_json::from_value(json!({
            "id": "f1", "rule": "duplicate-branches", "path": "z.go", "severity": "review",
            "message": "same branches",
            "start": { "line": 3, "column": 1 }, "end": { "line": 6, "column": 1 }
        }))
        .expect("finding"),
    ];
    report.duplication.clones = vec![clone(
        "c1",
        Category::Production,
        &[
            Region {
                path: "p.go",
                start: 1,
                end: 20,
                functions: &[],
            },
            Region {
                path: "q.go",
                start: 1,
                end: 20,
                functions: &[],
            },
        ],
    )];

    let entry = |id: &str, kind: &str, status: &str| {
        json!({ "id": id, "kind": kind, "fingerprint": "sha256:x", "status": status,
                "locations": [] })
    };
    report.review = Some(
        serde_json::from_value(json!({
            "schema_version": "1", "report_identity": "sha256:i", "source_manifest_digest": "sha256:m",
            "items": [
                entry("pkg/a.go::Retained", "function", "retained"),
                entry("pkg/b.go::Deferred", "function", "deferred"),
                entry("f1", "finding", "refactored"),
                entry("c1", "clone", "retained"),
            ],
            "counts": { "total": 4, "unreviewed": 0, "refactored": 1, "retained": 2,
                        "deferred": 1, "stale": 0 }
        }))
        .expect("ledger"),
    );
    let summary = summarize(&report, "repo");
    let titles: Vec<&str> = summary
        .worklist
        .iter()
        .map(|item| item.short_title.as_str())
        .collect();

    assert_eq!(summary.settled, 3);
    assert_eq!(titles.len(), 2, "{titles:?}");
    assert!(titles.contains(&"Deferred") && titles.contains(&"Open"));
    assert!(item(&summary, "Deferred").deferred);
    assert!(!item(&summary, "Open").deferred);
}

#[test]
fn advice_comes_from_the_first_matching_rule() {
    struct Case {
        name: &'static str,
        nesting: u32,
        nested_branches: u32,
        flat_guards: u32,
        field_mappings: u32,
        assertions: u32,
        test: bool,
        cyclomatic: u32,
        advice: &'static str,
    }

    let cases = [
        Case {
            name: "Deep",
            nesting: 4,
            nested_branches: 9,
            flat_guards: 9,
            field_mappings: 9,
            assertions: 9,
            test: false,
            cyclomatic: 11,
            advice: "Flatten it: turn the outer conditions into early returns or continues.",
        },
        Case {
            name: "Branchy",
            nesting: 3,
            nested_branches: 5,
            flat_guards: 9,
            field_mappings: 9,
            assertions: 9,
            test: false,
            cyclomatic: 11,
            advice: "Pull the inner branching into its own function.",
        },
        Case {
            name: "Guards",
            nesting: 3,
            nested_branches: 4,
            flat_guards: 4,
            field_mappings: 9,
            assertions: 9,
            test: false,
            cyclomatic: 11,
            advice: "Many guard clauses in a row; a lookup table may read better.",
        },
        Case {
            name: "Mapping",
            nesting: 3,
            nested_branches: 4,
            flat_guards: 3,
            field_mappings: 8,
            assertions: 9,
            test: false,
            cyclomatic: 11,
            advice: "Mostly field-by-field mapping; it may be fine as it is.",
        },
        Case {
            name: "Asserts",
            nesting: 3,
            nested_branches: 4,
            flat_guards: 3,
            field_mappings: 7,
            assertions: 8,
            test: true,
            cyclomatic: 11,
            advice: "Split it into table-driven cases.",
        },
        Case {
            name: "Marginal",
            nesting: 3,
            nested_branches: 4,
            flat_guards: 3,
            field_mappings: 7,
            assertions: 8,
            test: false,
            cyclomatic: 12,
            advice: "Just over the limit; leave it unless it grows.",
        },
        Case {
            name: "Plain",
            nesting: 3,
            nested_branches: 4,
            flat_guards: 3,
            field_mappings: 7,
            assertions: 8,
            test: false,
            cyclomatic: 13,
            advice: "Split it into smaller functions with one job each.",
        },
    ];
    let mut report = base_report();

    for (index, case) in cases.iter().enumerate() {
        let mut candidate = function(
            "a.go",
            case.name,
            u32::try_from(index).expect("small index") * 100 + 1,
            30,
            case.cyclomatic,
        );
        candidate.max_nesting = case.nesting;
        candidate.structural_signals.nested_branches = case.nested_branches;
        candidate.structural_signals.flat_guards = case.flat_guards;
        candidate.structural_signals.field_mappings = case.field_mappings;
        candidate.structural_signals.assertion_like_calls = case.assertions;

        if case.test {
            candidate.category = Category::Test;
        }

        report.functions.push(candidate);
    }

    let summary = summarize(&report, "repo");

    for case in &cases {
        assert_eq!(
            item(&summary, case.name).advice.as_deref(),
            Some(case.advice),
            "{}",
            case.name
        );
    }
}

#[test]
fn function_items_word_the_numbers_with_their_limit() {
    let mut report = base_report();
    let mut build = function("a.go", "buildResults", 1, 98, 16);
    build.max_nesting = 5;
    report.functions = vec![build];
    let summary = summarize(&report, "repo");
    let work = item(&summary, "buildResults");

    assert_eq!(work.title, "buildResults has 16 paths and nests 5 deep");
    assert!(work.why.contains("(limit 10)"), "{}", work.why);
    assert!(
        work.facts
            .iter()
            .any(|fact| fact.key == "Cyclomatic" && fact.value == "16 (limit 10)")
    );
    assert_eq!(
        work.why,
        "Cyclomatic complexity 16 (limit 10) over 98 lines; complexity weight 158."
    );
}

#[test]
fn parse_failures_become_a_hint() {
    let mut report = base_report();
    report.files = serde_json::from_value(json!([
        { "path": "broken.go", "category": "production", "status": "parse_error",
          "error": "syntax error", "bytes": 1, "lines": 1, "code_lines": 1 },
        { "path": "ok.go", "category": "production", "status": "analyzed",
          "bytes": 1, "lines": 1, "code_lines": 1 }
    ]))
    .expect("files");

    assert_eq!(
        summarize(&report, "repo").hints,
        ["1 file could not be parsed: broken.go."]
    );
}

#[test]
fn tooling_directories_trigger_the_category_skew_hint() {
    // karpenter-shaped: 4 of 6 over-limit functions live in tooling directories.
    let mut report = base_report();
    report.functions = vec![
        function("hack/price_validate/a.go", "A", 1, 40, 12),
        function("hack/price_validate/a.go", "B", 100, 40, 12),
        function("hack/price_validate/b.go", "C", 1, 40, 12),
        function("test/pkg/environment/env.go", "D", 1, 40, 12),
        function("pkg/core/x.go", "E", 1, 40, 12),
        function("pkg/core/y.go", "F", 1, 40, 12),
    ];
    let hints = summarize(&report, "repo").hints;

    assert_eq!(hints.len(), 1, "{hints:?}");
    assert!(
        hints[0].starts_with(
            "4 of the 6 over-limit functions are in hack/price_validate/ and test/pkg/environment/."
        ),
        "{}",
        hints[0]
    );
    assert!(hints[0].contains("marking them as tests"));

    // An even spread, or too few tooling functions, says nothing.
    report.functions[1].path = "pkg/core/z.go".to_owned();
    report.functions[2].path = "pkg/core/w.go".to_owned();
    assert!(summarize(&report, "repo").hints.is_empty());

    let mut lone = base_report();
    lone.functions = vec![function("hack/a.go", "A", 1, 40, 12)];
    assert!(summarize(&lone, "repo").hints.is_empty());
}

#[test]
fn copied_lines_in_tooling_directories_trigger_the_hint_too() {
    let mut report = base_report();
    report.duplication.clones = vec![
        clone(
            "tools",
            Category::Production,
            &[
                Region {
                    path: "scripts/a.go",
                    start: 1,
                    end: 20,
                    functions: &[],
                },
                Region {
                    path: "scripts/b.go",
                    start: 1,
                    end: 20,
                    functions: &[],
                },
            ],
        ),
        clone(
            "core",
            Category::Production,
            &[
                Region {
                    path: "core/a.go",
                    start: 1,
                    end: 10,
                    functions: &[],
                },
                Region {
                    path: "core/b.go",
                    start: 1,
                    end: 10,
                    functions: &[],
                },
            ],
        ),
    ];
    let hints = summarize(&report, "repo").hints;

    assert_eq!(hints.len(), 1, "{hints:?}");
    assert!(
        hints[0].starts_with("67% of copied production lines are in scripts/."),
        "{}",
        hints[0]
    );
}

#[test]
fn concentration_ranks_production_directories_by_weight() {
    let mut report = base_report();
    report.files = serde_json::from_value(json!([
        { "path": "heavy/a.go", "category": "production", "status": "analyzed",
          "bytes": 1, "lines": 500, "code_lines": 400 },
        { "path": "light/a.go", "category": "production", "status": "analyzed",
          "bytes": 1, "lines": 50, "code_lines": 40 }
    ]))
    .expect("files");
    report.functions = vec![
        function("heavy/a.go", "H", 1, 100, 20),
        function("light/a.go", "L", 1, 4, 2),
    ];
    let loads = summarize(&report, "repo").concentration;

    assert_eq!(loads[0].directory, "heavy/");
    assert_eq!(loads[0].weight, 200);
    assert_eq!(loads[0].over_limit, 1);
    assert_eq!(loads[0].code_lines, 400);
    assert_eq!(loads[1].directory, "light/");
    assert_eq!(loads[1].over_limit, 0);
}

#[test]
fn name_and_commit_come_through() {
    let summary = summarize(&base_report(), "karpenter");

    assert_eq!(summary.name, "karpenter");
    assert_eq!(summary.commit, "5b9aa47");
}
