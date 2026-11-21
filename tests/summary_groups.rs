//! Clone grouping and the comparison summary.

mod summary_fixture;

use deburr::compare::compare;
use deburr::render::summary::{WorkKind, summarize, summarize_comparison};
use deburr::report::Category;
use summary_fixture::{Region, base_report, clone, function, item};

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
                &[region(pair), region(pair + 1)],
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
        &[
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
fn overlapping_regions_in_one_file_read_as_one_place() {
    let mut report = base_report();
    report.functions = vec![
        function("a.go", "First", 10, 30, 3),
        function("a.go", "Second", 50, 30, 3),
    ];
    let first = Region {
        path: "a.go",
        start: 12,
        end: 20,
        functions: &["pkg/a.go::First"],
    };
    let first_longer = Region {
        path: "a.go",
        start: 12,
        end: 26,
        functions: &["pkg/a.go::First"],
    };
    let second = || Region {
        path: "a.go",
        start: 52,
        end: 66,
        functions: &["pkg/a.go::Second"],
    };
    report.duplication.clones = vec![
        clone("c1", Category::Production, &[first, second()]),
        clone("c2", Category::Production, &[first_longer, second()]),
    ];
    let summary = summarize(&report, "repo");
    let copied = summary
        .worklist
        .iter()
        .find(|entry| entry.kind == WorkKind::Copied)
        .expect("copied item");

    let places: Vec<(u32, u32)> = copied
        .locations
        .iter()
        .map(|place| (place.start.line, place.end.line))
        .collect();
    assert_eq!(places, [(12, 26), (52, 66)]);
    assert!(copied.why.contains("across 2 places"), "{}", copied.why);
}

#[test]
fn repeated_blocks_in_one_function_and_file_level_clones_get_their_own_titles() {
    let mut report = base_report();
    report.functions = vec![function("a.go", "Table", 1, 90, 3)];
    report.duplication.clones = vec![
        clone(
            "inside",
            Category::Production,
            &[
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
            &[
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
        &[
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
