//! Inline Rust tests: items under `#[cfg(test)]` and `#[test]` functions are
//! test code at function level, in metrics, and in duplication.
//!
//! The corpus fixture `corpus/rust/inline` has exact line counts; the other
//! scenarios are temporary trees.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use deburr::audit::{AuditOptions, AuditOutcome, audit};
use deburr::duplication::DuplicationSettings;
use deburr::report::{Category, DuplicationStatus, FileStatus, Report};

/// A function-level attribute and the category its function must get.
struct AttributeCase {
    attribute: &'static str,
    category: Category,
}

/// A function name and the category it must get.
struct NamedFunction {
    name: &'static str,
    category: Category,
}

fn options(include_tests: bool, duplication: Option<DuplicationSettings>) -> AuditOptions {
    AuditOptions {
        include_tests,
        include_testdata: false,
        include_vendor: false,
        include_generated: false,
        max_file_bytes: None,
        duplication,
    }
}

fn audit_root(root: &Path, options: &AuditOptions) -> AuditOutcome {
    audit(root, options, &mut io::sink())
}

fn corpus(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/corpus/rust")
        .join(name)
}

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent directories");
    }

    fs::write(path, contents).expect("write fixture");
}

fn category_of(report: &Report, name: &str) -> Category {
    report
        .functions
        .iter()
        .find(|function| function.name == name)
        .unwrap_or_else(|| panic!("function {name:?} not reported"))
        .category
}

#[test]
fn cfg_test_module_functions_are_test_functions() {
    let outcome = audit_root(&corpus("inline"), &options(true, None));
    let report = outcome.report;

    assert!(outcome.errors.is_empty(), "{:?}", outcome.errors);
    assert_eq!(report.coverage.parse_errors, 0);
    assert_eq!(category_of(&report, "double"), Category::Production);
    assert_eq!(category_of(&report, "tests::doubles"), Category::Test);
    // A file under `tests/` is test code in full.
    assert_eq!(category_of(&report, "outside"), Category::Test);

    let lib = report
        .files
        .iter()
        .find(|file| file.path == "src/lib.rs")
        .expect("src/lib.rs reported");

    // A file with inline tests stays production and analyzed.
    assert_eq!(lib.category, Category::Production);
    assert_eq!(lib.status, FileStatus::Analyzed);
    assert_eq!(lib.code_lines, 11);
    assert_eq!(lib.inline_test_code_lines, 8);

    let integration = report
        .files
        .iter()
        .find(|file| file.path == "tests/integration.rs")
        .expect("tests/integration.rs reported");

    assert_eq!(integration.code_lines, 4);
    assert_eq!(integration.inline_test_code_lines, 0);
}

#[test]
fn attributes_decide_which_functions_are_tests() {
    let cases = [
        AttributeCase {
            attribute: "#[test]",
            category: Category::Test,
        },
        AttributeCase {
            attribute: "#[tokio::test]",
            category: Category::Test,
        },
        AttributeCase {
            attribute: "#[tokio::test(flavor = \"multi_thread\")]",
            category: Category::Test,
        },
        AttributeCase {
            attribute: "#[sqlx::test(migrations = \"./m\")]",
            category: Category::Test,
        },
        AttributeCase {
            attribute: "#[cfg(test)]",
            category: Category::Test,
        },
        AttributeCase {
            attribute: "#[cfg(all(test, feature = \"x\"))]",
            category: Category::Test,
        },
        AttributeCase {
            attribute: "#[cfg(any(feature = \"x\", test))]",
            category: Category::Test,
        },
        AttributeCase {
            attribute: "#[cfg(all(unix, any(windows, test)))]",
            category: Category::Test,
        },
        AttributeCase {
            attribute: "#[cfg(not(test))]",
            category: Category::Production,
        },
        AttributeCase {
            attribute: "#[cfg(all(unix, not(test)))]",
            category: Category::Production,
        },
        AttributeCase {
            attribute: "#[cfg(feature = \"test\")]",
            category: Category::Production,
        },
        AttributeCase {
            attribute: "#[cfg_attr(test, derive(Debug))]",
            category: Category::Production,
        },
        AttributeCase {
            attribute: "#[test_case]",
            category: Category::Production,
        },
        AttributeCase {
            attribute: "#[tokio::main]",
            category: Category::Production,
        },
    ];
    let dir = tempfile::tempdir().expect("create temp dir");
    let mut source = String::new();

    for (index, case) in cases.iter().enumerate() {
        // A doc comment between the attribute and the item changes nothing.
        source.push_str(&format!(
            "{}\n/// Documented.\n#[inline]\nfn f{index}() {{\n    let _ = {index};\n}}\n\n",
            case.attribute
        ));
    }

    write(dir.path(), "src/lib.rs", &source);

    let report = audit_root(dir.path(), &options(true, None)).report;

    for (index, case) in cases.iter().enumerate() {
        assert_eq!(
            category_of(&report, &format!("f{index}")),
            case.category,
            "{}",
            case.attribute
        );
    }
}

#[test]
fn a_test_attribute_on_a_non_function_item_is_not_a_test_function() {
    let dir = tempfile::tempdir().expect("create temp dir");

    write(
        dir.path(),
        "src/lib.rs",
        "#[test]\nstruct Holder;\n\nimpl Holder {\n    fn method(&self) {\n        let _ = 1;\n    }\n}\n",
    );

    let report = audit_root(dir.path(), &options(true, None)).report;

    assert_eq!(category_of(&report, "Holder::method"), Category::Production);
    assert_eq!(report.files[0].inline_test_code_lines, 0);
}

#[test]
fn closures_and_nested_items_inside_a_test_region_are_test_functions() {
    let dir = tempfile::tempdir().expect("create temp dir");

    write(
        dir.path(),
        "src/lib.rs",
        "pub fn prod() {\n    let _ = || 1;\n}\n\n#[cfg(test)]\nmod tests {\n    fn helper() {\n        let _ = || 2;\n    }\n\n    #[test]\n    fn case() {\n        fn inner() {}\n        inner();\n    }\n}\n\nfn after() {}\n",
    );

    let report = audit_root(dir.path(), &options(true, None)).report;
    let expected = [
        NamedFunction {
            name: "prod",
            category: Category::Production,
        },
        NamedFunction {
            name: "prod::{closure#1}",
            category: Category::Production,
        },
        NamedFunction {
            name: "tests::helper",
            category: Category::Test,
        },
        NamedFunction {
            name: "tests::helper::{closure#1}",
            category: Category::Test,
        },
        NamedFunction {
            name: "tests::case",
            category: Category::Test,
        },
        NamedFunction {
            name: "tests::case::inner",
            category: Category::Test,
        },
        NamedFunction {
            name: "after",
            category: Category::Production,
        },
    ];

    for function in &expected {
        assert_eq!(
            category_of(&report, function.name),
            function.category,
            "{}",
            function.name
        );
    }

    assert_eq!(report.functions.len(), expected.len());
}

#[test]
fn bucket_totals_add_up() {
    let report = audit_root(&corpus("inline"), &options(true, None)).report;
    let metrics = &report.metrics;

    assert_eq!(metrics.production.code_lines, 3);
    assert_eq!(metrics.test.code_lines, 12);
    assert_eq!(metrics.all.code_lines, 15);
    assert_eq!(
        metrics.all.code_lines,
        metrics.production.code_lines + metrics.test.code_lines
    );
    assert_eq!(metrics.production.functions, 1);
    assert_eq!(metrics.test.functions, 2);
    assert_eq!(
        metrics.all.functions,
        metrics.production.functions + metrics.test.functions
    );
    assert_eq!(metrics.all.cyclomatic, 3);
}

#[test]
fn exclude_tests_drops_inline_tests_everywhere() {
    let outcome = audit_root(&corpus("inline"), &options(false, None));
    let report = outcome.report;

    assert!(outcome.errors.is_empty(), "{:?}", outcome.errors);

    let names: Vec<&str> = report
        .functions
        .iter()
        .map(|function| function.name.as_str())
        .collect();

    assert_eq!(names, ["double"]);

    // The file is still analyzed and still reports its own inline lines, but
    // those lines are in no bucket, `all` included.
    let lib = report
        .files
        .iter()
        .find(|file| file.path == "src/lib.rs")
        .expect("src/lib.rs reported");

    assert_eq!(lib.status, FileStatus::Analyzed);
    assert_eq!(lib.code_lines, 11);
    assert_eq!(lib.inline_test_code_lines, 8);
    assert_eq!(report.metrics.all.code_lines, 3);
    assert_eq!(report.metrics.production.code_lines, 3);
    assert_eq!(report.metrics.test.code_lines, 0);
    assert_eq!(report.metrics.test.functions, 0);
}

#[test]
fn production_identities_do_not_depend_on_exclude_tests() {
    let dir = tempfile::tempdir().expect("create temp dir");

    write(
        dir.path(),
        "src/lib.rs",
        "fn twin() {}\n\n#[test]\nfn twin_test() {}\n\nmod m {\n    pub fn twin() {}\n}\n",
    );

    let with = audit_root(dir.path(), &options(true, None)).report;
    let without = audit_root(dir.path(), &options(false, None)).report;
    let ids = |report: &Report| -> Vec<String> {
        report
            .functions
            .iter()
            .filter(|function| function.category == Category::Production)
            .map(|function| function.id.clone())
            .collect()
    };

    assert_eq!(ids(&with), ids(&without));
    assert_eq!(with.functions.len(), 3);
    assert_eq!(without.functions.len(), 2);
}

/// The same statements in every function, long enough to be a clone.
const SHARED_BODY: &str = "    let mut total = 0;
    for value in 0..10 {
        if value % 2 == 0 {
            total += value;
        } else {
            total -= 1;
        }
    }
    total
";

fn clone_settings() -> DuplicationSettings {
    DuplicationSettings {
        min_tokens: 20,
        min_lines: 3,
    }
}

#[test]
fn production_and_inline_test_code_never_pair() {
    let dir = tempfile::tempdir().expect("create temp dir");

    write(
        dir.path(),
        "src/lib.rs",
        &format!(
            "pub fn build() -> u32 {{\n{SHARED_BODY}}}\n\n#[cfg(test)]\nmod tests {{\n    #[test]\n    fn same() -> u32 {{\n{SHARED_BODY}    }}\n}}\n"
        ),
    );

    let report = audit_root(dir.path(), &options(true, Some(clone_settings()))).report;

    assert_eq!(report.duplication.status, DuplicationStatus::Measured);
    assert!(
        report.duplication.clones.is_empty(),
        "{:#?}",
        report.duplication.clones
    );
}

#[test]
fn clones_within_each_side_are_reported_in_that_side() {
    let dir = tempfile::tempdir().expect("create temp dir");

    write(
        dir.path(),
        "src/lib.rs",
        &format!(
            "pub fn first() -> u32 {{\n{SHARED_BODY}}}\n\npub fn second() -> u32 {{\n{SHARED_BODY}}}\n\n#[cfg(test)]\nmod tests {{\n    #[test]\n    fn one() -> u32 {{\n{SHARED_BODY}    }}\n\n    #[test]\n    fn two() -> u32 {{\n{SHARED_BODY}    }}\n}}\n"
        ),
    );

    let report = audit_root(dir.path(), &options(true, Some(clone_settings()))).report;
    let clones = &report.duplication.clones;

    assert_eq!(report.duplication.status, DuplicationStatus::Measured);
    assert_eq!(clones.len(), 2, "{clones:#?}");

    for clone in clones {
        let wanted = match clone.category {
            Category::Production => ["first", "second"],
            Category::Test => ["tests::one", "tests::two"],
            other => panic!("unexpected clone category {other:?}"),
        };
        let mut owners: Vec<&str> = clone
            .endpoints
            .iter()
            .flat_map(|endpoint| &endpoint.functions)
            .map(|id| {
                report
                    .functions
                    .iter()
                    .find(|function| &function.id == id)
                    .map(|function| function.name.as_str())
                    .expect("clone function is reported")
            })
            .collect();

        owners.sort_unstable();
        assert_eq!(owners, wanted, "{:?}", clone.category);

        // Endpoints keep real source positions: `#[cfg(test)]` is on line 25,
        // so a test clone starts after it and a production clone before it.
        for endpoint in &clone.endpoints {
            assert_eq!(endpoint.path, "src/lib.rs");
            assert_eq!(
                endpoint.start.line >= 25,
                clone.category == Category::Test,
                "{endpoint:?}"
            );
        }
    }
}

#[test]
fn duplication_categories_cover_the_split_code_lines() {
    let dir = tempfile::tempdir().expect("create temp dir");

    write(
        dir.path(),
        "src/lib.rs",
        &format!(
            "pub fn first() -> u32 {{\n{SHARED_BODY}}}\n\npub fn second() -> u32 {{\n{SHARED_BODY}}}\n\n#[cfg(test)]\nmod tests {{\n    #[test]\n    fn one() -> u32 {{\n{SHARED_BODY}    }}\n}}\n"
        ),
    );

    let report = audit_root(dir.path(), &options(true, Some(clone_settings()))).report;
    let category = |wanted: Category| {
        report
            .duplication
            .categories
            .iter()
            .find(|row| row.category == wanted)
            .expect("category measured")
    };

    assert_eq!(
        category(Category::Production).code_lines,
        report.metrics.production.code_lines
    );
    assert_eq!(
        category(Category::Test).code_lines,
        report.metrics.test.code_lines
    );
    assert_eq!(category(Category::Production).files, 1);
    // The file with inline tests is also in the test group.
    assert_eq!(category(Category::Test).files, 1);
    // Both production functions are fully duplicated; the lone test is not.
    assert_eq!(
        category(Category::Production).duplicated_code_lines,
        category(Category::Production).code_lines
    );
    assert_eq!(category(Category::Test).duplicated_code_lines, 0);
}

#[test]
fn exclude_tests_keeps_inline_tests_out_of_duplication() {
    let dir = tempfile::tempdir().expect("create temp dir");

    write(
        dir.path(),
        "src/lib.rs",
        &format!(
            "pub fn first() -> u32 {{\n{SHARED_BODY}}}\n\n#[cfg(test)]\nmod tests {{\n    #[test]\n    fn one() -> u32 {{\n{SHARED_BODY}    }}\n}}\n"
        ),
    );

    let report = audit_root(dir.path(), &options(false, Some(clone_settings()))).report;
    let production = report
        .duplication
        .categories
        .iter()
        .find(|row| row.category == Category::Production)
        .expect("production measured");
    let test = report
        .duplication
        .categories
        .iter()
        .find(|row| row.category == Category::Test)
        .expect("test measured");

    assert!(report.duplication.clones.is_empty());
    assert_eq!(production.code_lines, report.metrics.production.code_lines);
    assert_eq!(test.files, 0);
    assert_eq!(test.code_lines, 0);
}

#[test]
fn a_file_of_only_test_code_is_only_in_the_test_group() {
    let dir = tempfile::tempdir().expect("create temp dir");

    write(
        dir.path(),
        "src/only.rs",
        &format!(
            "#[cfg(test)]\nmod tests {{\n    #[test]\n    fn one() -> u32 {{\n{SHARED_BODY}    }}\n\n    #[test]\n    fn two() -> u32 {{\n{SHARED_BODY}    }}\n}}\n"
        ),
    );

    let report = audit_root(dir.path(), &options(true, Some(clone_settings()))).report;
    let production = report
        .duplication
        .categories
        .iter()
        .find(|row| row.category == Category::Production)
        .expect("production measured");
    let test = report
        .duplication
        .categories
        .iter()
        .find(|row| row.category == Category::Test)
        .expect("test measured");

    assert_eq!(production.code_lines, 0);
    assert_eq!(test.files, 1);
    assert_eq!(test.code_lines, report.metrics.test.code_lines);
    assert_eq!(report.metrics.production.code_lines, 0);
    assert_eq!(report.duplication.clones.len(), 1);
    assert_eq!(report.duplication.clones[0].category, Category::Test);
}
