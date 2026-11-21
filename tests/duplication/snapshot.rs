//! A pinned snapshot of the detector's output over a fixed fixture, so a
//! `cpd-core` upgrade that changes results fails here.

use std::path::{Path, PathBuf};

use deburr::report::Category;

use super::{PERCENT_TOLERANCE, assert_functions_resolve, run_default};

/// A line and column span, 1-based with inclusive end.
struct Span {
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
}

struct ExpectedEndpoint {
    path: &'static str,
    span: Span,
    code_lines: u32,
    functions: &'static [&'static str],
}

struct ExpectedClone {
    id: &'static str,
    family_id: &'static str,
    category: Category,
    tokens: u32,
    endpoints: [ExpectedEndpoint; 2],
}

struct ExpectedMeasurement {
    category: Category,
    files: u32,
    code_lines: u32,
    duplicated_code_lines: u32,
}

fn snapshot_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus/dup/snapshot")
}

/// Pins every clone found in a fixed Go and TypeScript fixture: IDs, token
/// counts, spans, code lines, and the functions named. A `cpd-core` upgrade
/// (or a token or identity change) that alters detection fails here, so it
/// is reviewed rather than slipping into reports.
#[test]
fn snapshot_of_clones_over_the_fixture() {
    let report = run_default(&snapshot_root());
    let expected = [
        ExpectedClone {
            id: "clone-004d7fd80e94a89a",
            family_id: "family-4b3a4f336a0ac6d7",
            category: Category::Production,
            tokens: 108,
            endpoints: [
                ExpectedEndpoint {
                    path: "go/alpha.go",
                    span: Span {
                        start_line: 6,
                        start_column: 1,
                        end_line: 29,
                        end_column: 1,
                    },
                    code_lines: 24,
                    functions: &["alpha/go/alpha.go::Normalize"],
                },
                ExpectedEndpoint {
                    path: "go/beta.go",
                    span: Span {
                        start_line: 9,
                        start_column: 1,
                        end_line: 36,
                        end_column: 1,
                    },
                    code_lines: 24,
                    functions: &["beta/go/beta.go::Normalize"],
                },
            ],
        },
        ExpectedClone {
            id: "clone-4a38c1fa4bdeca9c",
            family_id: "family-74e2dc1a262ad6b1",
            category: Category::Test,
            tokens: 132,
            endpoints: [
                ExpectedEndpoint {
                    path: "go/beta_test.go",
                    span: Span {
                        start_line: 5,
                        start_column: 1,
                        end_line: 23,
                        end_column: 1,
                    },
                    code_lines: 18,
                    functions: &["beta/go/beta_test.go::TestNormalizeRange"],
                },
                ExpectedEndpoint {
                    path: "go/alpha_test.go",
                    span: Span {
                        start_line: 5,
                        start_column: 1,
                        end_line: 23,
                        end_column: 1,
                    },
                    code_lines: 18,
                    functions: &["alpha/go/alpha_test.go::TestNormalizeRange"],
                },
            ],
        },
        ExpectedClone {
            id: "clone-a0a2eee1b1638c05",
            family_id: "family-df551a656716e50d",
            category: Category::Production,
            tokens: 156,
            endpoints: [
                ExpectedEndpoint {
                    path: "ts/two.ts",
                    span: Span {
                        start_line: 9,
                        start_column: 1,
                        end_line: 25,
                        end_column: 1,
                    },
                    code_lines: 17,
                    functions: &["typescript/ts/two.ts::summarize"],
                },
                ExpectedEndpoint {
                    path: "ts/one.ts",
                    span: Span {
                        start_line: 10,
                        start_column: 1,
                        end_line: 26,
                        end_column: 1,
                    },
                    code_lines: 17,
                    functions: &["typescript/ts/one.ts::summarize"],
                },
            ],
        },
    ];
    let measurements = [
        ExpectedMeasurement {
            category: Category::Production,
            files: 4,
            code_lines: 111,
            duplicated_code_lines: 82,
        },
        ExpectedMeasurement {
            category: Category::Test,
            files: 2,
            code_lines: 43,
            duplicated_code_lines: 36,
        },
    ];

    assert_eq!(report.duplication.clones.len(), expected.len());

    for (clone, pinned) in report.duplication.clones.iter().zip(&expected) {
        assert_eq!(clone.id, pinned.id);
        assert_eq!(clone.family_id, pinned.family_id);
        assert_eq!(clone.category, pinned.category, "{}", pinned.id);
        assert_eq!(clone.tokens, pinned.tokens, "{}", pinned.id);
        assert!(!clone.identity_ambiguous, "{}", pinned.id);

        for (endpoint, pinned_endpoint) in clone.endpoints.iter().zip(&pinned.endpoints) {
            let span = &pinned_endpoint.span;

            assert_eq!(endpoint.path, pinned_endpoint.path, "{}", pinned.id);
            assert_eq!(
                (
                    endpoint.start.line,
                    endpoint.start.column,
                    endpoint.end.line,
                    endpoint.end.column
                ),
                (
                    span.start_line,
                    span.start_column,
                    span.end_line,
                    span.end_column
                ),
                "{} {}",
                pinned.id,
                endpoint.path
            );
            assert_eq!(
                endpoint.code_lines, pinned_endpoint.code_lines,
                "{}",
                pinned.id
            );
            assert_eq!(
                endpoint.functions, pinned_endpoint.functions,
                "{}",
                pinned.id
            );
        }
    }

    assert_eq!(report.duplication.categories.len(), measurements.len());

    for (row, pinned) in report.duplication.categories.iter().zip(&measurements) {
        assert_eq!(row.category, pinned.category);
        assert_eq!(row.files, pinned.files, "{:?}", row.category);
        assert_eq!(row.code_lines, pinned.code_lines, "{:?}", row.category);
        assert_eq!(
            row.duplicated_code_lines, pinned.duplicated_code_lines,
            "{:?}",
            row.category
        );
        assert!(
            (row.percentage
                - f64::from(pinned.duplicated_code_lines) / f64::from(pinned.code_lines) * 100.0)
                .abs()
                < PERCENT_TOLERANCE,
            "{:?}",
            row.category
        );
    }

    assert_functions_resolve(&report);
}

#[test]
fn clones_are_the_same_on_every_run() {
    let first = run_default(&snapshot_root());
    let second = run_default(&snapshot_root());

    assert_eq!(first.duplication, second.duplication);
}
