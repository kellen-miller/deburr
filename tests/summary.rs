//! The shared human summary: verdict, hints, grouping, ranking, review
//! filtering, advice, and number wording.

use deburr::compare::compare;
use deburr::render::summary::{
    FactState, Summary, WorkItem, WorkKind, count, percent, summarize, summarize_comparison,
};
use deburr::report::{Category, CodeClone, Function, Report};
use serde_json::json;

/// A clone endpoint: a region of `path` and the functions it overlaps.
struct Region<'a> {
    path: &'a str,
    start: u32,
    end: u32,
    functions: &'a [&'a str],
}

fn base_report() -> Report {
    serde_json::from_value(json!({
        "duplication": {
            "status": "measured", "min_tokens": 80, "min_lines": 8,
            "categories": [], "clones": []
        },
        "provenance": {
            "source": { "commit": "5b9aa471e41eb63747142b2247157290ae911902",
                        "manifest_digest": "sha256:m", "dirty": false, "git_known": true },
            "build": { "version": "1.0.0" }
        },
        "analyzer": { "id": "deburr", "version": "1.0.0" },
        "schema_version": "3",
        "files": [],
        "functions": [],
        "findings": [],
        "scope": { "path_mode": "source-relative", "languages": ["go"],
                   "include_tests": true, "include_testdata": false,
                   "include_vendor": false, "include_generated": false },
        "metrics": {
            "all": zero_bucket(), "production": zero_bucket(), "test": zero_bucket()
        },
        "config": {
            "rules": [{ "id": "duplicate-branches", "enabled": true },
                      { "id": "redundant-boolean-return", "enabled": true }],
            "language": "go",
            "duplication": { "requested": true, "min_tokens": 80, "min_lines": 8 },
            "high_complexity_threshold": 10,
            "include_tests": true, "include_testdata": false,
            "include_vendor": false, "include_generated": false
        },
        "coverage": {
            "by_category": [], "discovered": 1, "analyzed": 1, "excluded": 0,
            "unsupported": 0, "read_errors": 0, "parse_errors": 0, "symlinks": 0,
            "excluded_directories": 0
        }
    }))
    .expect("report")
}

fn zero_bucket() -> serde_json::Value {
    json!({
        "high_complexity_mass_share": null, "code_lines": 0, "functions": 0, "cyclomatic": 0,
        "max_nesting": 0, "mass": 0.0, "high_complexity_functions": 0, "high_complexity_mass": 0.0
    })
}

/// A production function `path::name` of `sloc` lines starting at `line`.
fn function(path: &str, name: &str, line: u32, sloc: u32, cyclomatic: u32) -> Function {
    serde_json::from_value(json!({
        "id": format!("pkg/{path}::{name}"),
        "fingerprint": format!("sha256:{path}:{name}"),
        "path": path,
        "category": "production",
        "name": name,
        "structural_signals": {
            "flat_guards": 0, "field_mappings": 0, "nested_branches": 0, "assertion_like_calls": 0
        },
        "start": { "line": line, "column": 1 },
        "end": { "line": line + sloc, "column": 2 },
        "sloc": sloc,
        "cyclomatic": cyclomatic,
        "max_nesting": 1,
        "mass": f64::from(cyclomatic) * f64::from(sloc).sqrt(),
        "high_complexity": cyclomatic > 10,
    }))
    .expect("function")
}

fn clone(id: &str, category: Category, regions: [Region<'_>; 2]) -> CodeClone {
    let endpoint = |region: &Region<'_>| {
        json!({
            "path": region.path,
            "start": { "line": region.start, "column": 1 },
            "end": { "line": region.end, "column": 2 },
            "code_lines": region.end - region.start + 1,
            "functions": region.functions,
        })
    };

    serde_json::from_value(json!({
        "id": id, "family_id": format!("family-{id}"), "fingerprint": format!("sha256:{id}"),
        "category": category.as_str(), "tokens": 100,
        "endpoints": [endpoint(&regions[0]), endpoint(&regions[1])]
    }))
    .expect("clone")
}

fn item<'a>(summary: &'a Summary, short_title: &str) -> &'a WorkItem {
    summary
        .worklist
        .iter()
        .find(|item| item.short_title == short_title)
        .unwrap_or_else(|| panic!("no worklist item {short_title:?}: {:?}", summary.worklist))
}

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
fn clone_pairs_that_share_a_helper_collapse_into_one_group() {
    // The infra-scout shape: one helper copied into 6 files, one of which
    // holds a second function, joined by only 5 pairs.
    let mut report = base_report();

    for index in 0..6 {
        report.functions.push(function(
            &format!("d{index}.go"),
            "importFindings",
            10,
            9,
            3,
        ));
    }

    report.functions.push(function("d1.go", "helper", 20, 9, 3));

    // Six regions; the second overlaps two functions.
    let paths = ["d0.go", "d1.go", "d2.go", "d3.go", "d4.go", "d5.go"];
    let owners: [&[&str]; 6] = [
        &["pkg/d0.go::importFindings"],
        &["pkg/d1.go::importFindings", "pkg/d1.go::helper"],
        &["pkg/d2.go::importFindings"],
        &["pkg/d3.go::importFindings"],
        &["pkg/d4.go::importFindings"],
        &["pkg/d5.go::importFindings"],
    ];
    let region = |index: usize| Region {
        path: paths[index],
        start: 10,
        end: 29,
        functions: owners[index],
    };

    report.duplication.clones = (0..5)
        .map(|pair| {
            clone(
                &format!("c{pair}"),
                Category::Production,
                [region(pair), region(pair + 1)],
            )
        })
        .collect();

    let summary = summarize(&report, "repo");

    assert_eq!(summary.groups.len(), 1, "{:?}", summary.groups);

    let group = &summary.groups[0];
    assert_eq!(group.clones, ["c0", "c1", "c2", "c3", "c4"]);
    assert_eq!(group.functions.len(), 7);
    assert_eq!(group.copied_lines, 6 * 20, "each region counts once");
    assert_eq!(group.title, "importFindings and 6 variants");
    assert_eq!(
        summary.worklist[0].title,
        "importFindings is copied into 7 variants"
    );
}

#[test]
fn closures_inside_a_listed_function_do_not_own_the_clone() {
    let mut report = base_report();
    report.functions = vec![
        function("a.go", "Outer", 10, 30, 3),
        function("a.go", "Outer.anon-ab12", 15, 5, 1),
        function("b.go", "Other", 10, 30, 3),
    ];
    report.duplication.clones = vec![clone(
        "c1",
        Category::Production,
        [
            Region {
                path: "a.go",
                start: 12,
                end: 30,
                functions: &["pkg/a.go::Outer", "pkg/a.go::Outer.anon-ab12"],
            },
            Region {
                path: "b.go",
                start: 12,
                end: 30,
                functions: &["pkg/b.go::Other"],
            },
        ],
    )];
    let group = &summarize(&report, "repo").groups[0];

    assert_eq!(group.functions, ["pkg/a.go::Outer", "pkg/b.go::Other"]);
    assert_eq!(group.title, "Outer / Other");
}

#[test]
fn repeated_blocks_in_one_function_and_file_level_clones_get_their_own_titles() {
    let mut report = base_report();
    report.functions = vec![function("a.go", "Table", 1, 90, 3)];
    report.duplication.clones = vec![
        clone(
            "inside",
            Category::Production,
            [
                Region {
                    path: "a.go",
                    start: 5,
                    end: 20,
                    functions: &["pkg/a.go::Table"],
                },
                Region {
                    path: "a.go",
                    start: 30,
                    end: 45,
                    functions: &["pkg/a.go::Table"],
                },
            ],
        ),
        clone(
            "file-level",
            Category::Production,
            [
                Region {
                    path: "x.go",
                    start: 1,
                    end: 10,
                    functions: &[],
                },
                Region {
                    path: "y.go",
                    start: 1,
                    end: 10,
                    functions: &[],
                },
            ],
        ),
    ];
    let summary = summarize(&report, "repo");

    let inside = item(&summary, "Table repeats one block 2 times inside itself");
    assert_eq!(
        inside.advice.as_deref(),
        Some("Loop or table-drive the repeated block.")
    );

    let file_level = item(&summary, "Copied code in x.go and 1 other file");
    assert_eq!(file_level.kind, WorkKind::Copied);
    assert_eq!(file_level.size, 20);
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
            [
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
            [
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
            [
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
            [
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
            [
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
            [
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
        [
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
            [
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
            [
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

#[test]
fn comparison_summary_lists_what_got_worse_and_better() {
    let mut before = base_report();
    before.functions = vec![function("a.go", "Grew", 1, 30, 9)];
    let mut after = base_report();
    after.functions = vec![function("a.go", "Grew", 1, 30, 11)];

    let worse = summarize_comparison(&compare(&before, &after));
    assert!(worse.conclusive);
    assert_eq!(worse.verdict, "1 got worse, 0 improved");
    assert_eq!(worse.worse[0].title, "Grew");
    assert_eq!(worse.worse[0].before, "cyclomatic 9");
    assert_eq!(worse.worse[0].after, "cyclomatic 11");
    assert_eq!(worse.worse[0].kind, WorkKind::Complex);

    let better = summarize_comparison(&compare(&after, &before));
    assert_eq!(better.verdict, "0 got worse, 1 improved");
    assert_eq!(better.better[0].before, "cyclomatic 11");
    assert_eq!(better.better[0].after, "cyclomatic 9");
}

#[test]
fn comparison_summary_groups_new_and_removed_clones() {
    let mut before = base_report();
    let mut after = base_report();
    after.duplication.clones = vec![clone(
        "c1",
        Category::Production,
        [
            Region {
                path: "a.go",
                start: 1,
                end: 20,
                functions: &["pkg/a.go::One"],
            },
            Region {
                path: "b.go",
                start: 1,
                end: 20,
                functions: &["pkg/b.go::Two"],
            },
        ],
    )];

    let added = summarize_comparison(&compare(&before, &after));
    assert_eq!(added.verdict, "1 got worse, 0 improved");
    assert_eq!(added.worse[0].kind, WorkKind::Copied);
    assert_eq!(added.worse[0].title, "One / Two");
    assert_eq!(added.worse[0].after, "40 copied lines");

    before.duplication.clones = after.duplication.clones.clone();
    after.duplication.clones.clear();
    let removed = summarize_comparison(&compare(&before, &after));
    assert_eq!(removed.verdict, "0 got worse, 1 improved");
    assert_eq!(removed.better[0].title, "One / Two");
}

#[test]
fn comparison_summary_refuses_to_conclude_when_incompatible() {
    let before = base_report();
    let mut after = base_report();
    after.duplication.min_tokens = 50;
    after.config.duplication.min_tokens = 50;
    let comparison = compare(&before, &after);
    let summary = summarize_comparison(&comparison);

    assert!(!summary.conclusive, "{:?}", comparison.reason);
    assert!(
        summary.verdict.starts_with("No conclusion: "),
        "{}",
        summary.verdict
    );
    assert!(summary.worse.is_empty() && summary.better.is_empty());
}
