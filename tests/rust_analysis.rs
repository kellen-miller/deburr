//! Rust analysis: exact measurements over the fixtures in `corpus/rust/`,
//! plus temporary trees for names, discovery, and parse errors.
//!
//! There is no reference implementation for Rust, so these assertions are the
//! specification. Positions are 1-based with byte columns. Each scenario is a
//! directory audited as its own root.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use deburr::audit::{AuditOptions, AuditOutcome, audit};
use deburr::report::{Category, FileStatus, Function, Position, Report};

/// The exact measurements asserted for one function.
struct Expected {
    name: &'static str,
    start: Position,
    end: Position,
    sloc: u32,
    cyclomatic: u32,
    max_nesting: u32,
    nested_branches: u32,
}

/// What discovery should say about one file.
struct ExpectedFile {
    path: &'static str,
    category: Category,
    status: FileStatus,
}

const fn at(line: u32, column: u32) -> Position {
    Position { line, column }
}

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus/rust")
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

fn tree() -> tempfile::TempDir {
    tempfile::tempdir().expect("create temp dir")
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

fn names(report: &Report) -> Vec<&str> {
    report.functions.iter().map(|f| f.name.as_str()).collect()
}

fn assert_file(report: &Report, expected: &ExpectedFile) {
    let file = report
        .files
        .iter()
        .find(|file| file.path == expected.path)
        .unwrap_or_else(|| panic!("file {:?} not reported", expected.path));

    assert_eq!(
        file.category, expected.category,
        "{} category",
        expected.path
    );
    assert_eq!(file.status, expected.status, "{} status", expected.path);
}

#[test]
fn constructs_have_exact_measurements() {
    let report = scenario("constructs");
    let expected = [
        Expected {
            name: "plain",
            start: at(5, 1),
            end: at(7, 1),
            sloc: 3,
            cyclomatic: 1,
            max_nesting: 0,
            nested_branches: 0,
        },
        // if (+1), the let chain's two `&&` (+2), `else if` (+1), `||` (+1);
        // the `else if` does not nest deeper than its head.
        Expected {
            name: "branches",
            start: at(9, 1),
            end: at(20, 1),
            sloc: 12,
            cyclomatic: 6,
            max_nesting: 1,
            nested_branches: 0,
        },
        // Three arms and one guard; the catch-all `_` adds nothing.
        Expected {
            name: "matching",
            start: at(22, 1),
            end: at(29, 1),
            sloc: 8,
            cyclomatic: 5,
            max_nesting: 1,
            nested_branches: 0,
        },
        // `_ if cond` is a guarded arm, not the catch-all: arm and guard.
        Expected {
            name: "guarded_default",
            start: at(31, 1),
            end: at(37, 1),
            sloc: 7,
            cyclomatic: 4,
            max_nesting: 1,
            nested_branches: 0,
        },
        Expected {
            name: "fallible",
            start: at(39, 1),
            end: at(43, 1),
            sloc: 5,
            cyclomatic: 3,
            max_nesting: 0,
            nested_branches: 0,
        },
        Expected {
            name: "let_else",
            start: at(45, 1),
            end: at(50, 1),
            sloc: 6,
            cyclomatic: 2,
            max_nesting: 0,
            nested_branches: 0,
        },
        // `loop` nests but adds no decision.
        Expected {
            name: "loops",
            start: at(52, 1),
            end: at(65, 1),
            sloc: 14,
            cyclomatic: 4,
            max_nesting: 4,
            nested_branches: 3,
        },
        // Closure bodies belong to the closures, not to the parent.
        Expected {
            name: "with_closures",
            start: at(67, 1),
            end: at(78, 1),
            sloc: 3,
            cyclomatic: 1,
            max_nesting: 0,
            nested_branches: 0,
        },
        Expected {
            name: "with_closures::{closure#1}",
            start: at(68, 46),
            end: at(68, 54),
            sloc: 1,
            cyclomatic: 1,
            max_nesting: 0,
            nested_branches: 0,
        },
        Expected {
            name: "with_closures::{closure#2}",
            start: at(69, 39),
            end: at(72, 5),
            sloc: 4,
            cyclomatic: 2,
            max_nesting: 0,
            nested_branches: 0,
        },
        Expected {
            name: "with_closures::{closure#3}",
            start: at(73, 18),
            end: at(76, 5),
            sloc: 3,
            cyclomatic: 1,
            max_nesting: 0,
            nested_branches: 0,
        },
        Expected {
            name: "with_closures::{closure#3}::{closure#1}",
            start: at(74, 21),
            end: at(74, 34),
            sloc: 1,
            cyclomatic: 1,
            max_nesting: 0,
            nested_branches: 0,
        },
        Expected {
            name: "outer",
            start: at(80, 1),
            end: at(85, 1),
            sloc: 3,
            cyclomatic: 1,
            max_nesting: 0,
            nested_branches: 0,
        },
        Expected {
            name: "outer::inner",
            start: at(81, 5),
            end: at(83, 5),
            sloc: 3,
            cyclomatic: 2,
            max_nesting: 1,
            nested_branches: 0,
        },
        Expected {
            name: "Wrapper::new",
            start: at(92, 5),
            end: at(94, 5),
            sloc: 3,
            cyclomatic: 1,
            max_nesting: 0,
            nested_branches: 0,
        },
        Expected {
            name: "Wrapper::get",
            start: at(96, 5),
            end: at(98, 5),
            sloc: 3,
            cyclomatic: 1,
            max_nesting: 0,
            nested_branches: 0,
        },
        Expected {
            name: "<Wrapper as fmt::Display>::fmt",
            start: at(102, 5),
            end: at(104, 5),
            sloc: 3,
            cyclomatic: 1,
            max_nesting: 0,
            nested_branches: 0,
        },
        // A trait's default body is a unit; `fn area(&self);` is not.
        Expected {
            name: "Shape::describe",
            start: at(110, 5),
            end: at(116, 5),
            sloc: 7,
            cyclomatic: 2,
            max_nesting: 1,
            nested_branches: 0,
        },
        Expected {
            name: "geometry::circle::radius",
            start: at(121, 9),
            end: at(123, 9),
            sloc: 3,
            cyclomatic: 2,
            max_nesting: 1,
            nested_branches: 0,
        },
        Expected {
            name: "geometry::super::Wrapper::double",
            start: at(127, 9),
            end: at(129, 9),
            sloc: 3,
            cyclomatic: 1,
            max_nesting: 0,
            nested_branches: 0,
        },
        Expected {
            name: "HANDLER::{closure#1}",
            start: at(133, 37),
            end: at(133, 45),
            sloc: 1,
            cyclomatic: 1,
            max_nesting: 0,
            nested_branches: 0,
        },
        Expected {
            name: "generic",
            start: at(135, 1),
            end: at(137, 1),
            sloc: 3,
            cyclomatic: 1,
            max_nesting: 0,
            nested_branches: 0,
        },
    ];

    // `area` has no body and `twice!` is a macro body: neither is measured.
    assert_eq!(
        report.functions.len(),
        expected.len(),
        "{:?}",
        names(&report)
    );

    for want in &expected {
        let found = function(&report, want.name);

        assert_eq!(found.id, format!("rust/src/lib.rs::{}", want.name));
        assert_eq!(found.path, "src/lib.rs", "{} path", want.name);
        assert_eq!(found.category, Category::Production, "{}", want.name);
        assert_eq!(found.start, want.start, "{} start", want.name);
        assert_eq!(found.end, want.end, "{} end", want.name);
        assert_eq!(found.sloc, want.sloc, "{} sloc", want.name);
        assert_eq!(
            found.cyclomatic, want.cyclomatic,
            "{} cyclomatic",
            want.name
        );
        assert_eq!(found.max_nesting, want.max_nesting, "{} nesting", want.name);
        assert_eq!(
            found.structural_signals.nested_branches, want.nested_branches,
            "{} nested branches",
            want.name
        );
        assert!(!found.identity_ambiguous, "{} ambiguity", want.name);
        assert!(!found.high_complexity, "{} high complexity", want.name);
    }

    assert!(
        report.findings.is_empty(),
        "constructs has no finding shape: {:?}",
        report.findings
    );
}

#[test]
fn schema_four_fields_describe_the_constructs_fixture() {
    let report = scenario("constructs");

    assert_eq!(report.schema_version, "4");
    assert_eq!(report.scope.languages, ["go", "typescript", "rust"]);
    assert_eq!(report.config.language, "go,typescript,rust");
    assert_eq!(report.coverage.parse_errors, 0);

    let file = &report.files[0];

    assert_eq!(file.inline_test_code_lines, 0);
    assert!(file.code_lines > 100, "code lines {}", file.code_lines);
    assert_eq!(report.metrics.production.code_lines, file.code_lines);
    assert_eq!(report.metrics.production.functions, 22);

    // `Shape::describe` is the only function with a clone-like call: two
    // `.to_owned()` calls. The Go-only signals stay zero for Rust.
    for found in &report.functions {
        let signals = &found.structural_signals;
        let clone_like = if found.name == "Shape::describe" {
            2
        } else {
            0
        };

        assert_eq!(
            [
                signals.clone_like_calls,
                signals.unwrap_like_calls,
                signals.panic_macros,
                signals.unsafe_blocks,
                signals.flat_guards,
                signals.field_mappings,
                signals.assertion_like_calls,
            ],
            [clone_like, 0, 0, 0, 0, 0, 0],
            "{} signals",
            found.name
        );
    }
}

#[test]
fn high_complexity_follows_the_shared_threshold() {
    let root = tree();
    let mut body = String::from("fn busy(x: u32) -> u32 {\n");

    for index in 0..10 {
        body.push_str(&format!("    if x == {index} {{ return {index}; }}\n"));
    }

    body.push_str("    0\n}\n");
    write(root.path(), "src/lib.rs", &body);

    let report = audit_root(root.path()).report;
    let busy = function(&report, "busy");

    assert_eq!(busy.cyclomatic, 11);
    assert!(busy.high_complexity);
    assert_eq!(report.metrics.production.high_complexity_functions, 1);
}

#[test]
fn names_follow_mod_impl_and_trait_containers() {
    let root = tree();

    write(
        root.path(),
        "src/lib.rs",
        "mod a {
    pub struct X;

    impl X {
        pub fn go(&self) -> u32 {
            let f = || 1;
            f()
        }
    }
}

impl Iterator for Wrapper<u8> {
    type Item = u8;

    fn next(&mut self) -> Option<u8> {
        None
    }
}

impl<'a> Foo for &'a Bar {
    fn x(&self) {}
}

impl std::fmt::Debug for crate::Thing<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}

pub fn free() {
    const LIMIT: fn() -> u32 = || 7;
    let g = || LIMIT();
    g();
}

pub async fn runner() {
    let task = async move || 1;
    let other = move |a: u32| a;
}
",
    );

    let report = audit_root(root.path()).report;

    assert_eq!(
        names(&report),
        [
            "a::X::go",
            "a::X::go::{closure#1}",
            "<Wrapper as Iterator>::next",
            "<&'a Bar as Foo>::x",
            "<crate::Thing as std::fmt::Debug>::fmt",
            "free",
            "free::LIMIT::{closure#1}",
            "free::{closure#1}",
            "runner",
            "runner::{closure#1}",
            "runner::{closure#2}",
        ]
    );
    assert!(
        report
            .functions
            .iter()
            .all(|f| f.id.starts_with("rust/src/lib.rs::"))
    );
}

#[test]
fn repeated_names_get_numbered_identities_and_are_ambiguous() {
    let root = tree();

    write(
        root.path(),
        "src/lib.rs",
        "pub struct Foo;

#[cfg(unix)]
impl Foo {
    pub fn run(&self) -> u32 {
        let f = || 1;
        f()
    }
}

#[cfg(not(unix))]
impl Foo {
    pub fn run(&self) -> u32 {
        let f = || 2;
        f()
    }
}

pub fn unique() {}
",
    );

    let report = audit_root(root.path()).report;
    let ids: Vec<&str> = report.functions.iter().map(|f| f.id.as_str()).collect();

    assert_eq!(
        ids,
        [
            "rust/src/lib.rs::Foo::run",
            "rust/src/lib.rs::Foo::run::{closure#1}",
            "rust/src/lib.rs::Foo::run#2",
            "rust/src/lib.rs::Foo::run::{closure#1}#2",
            "rust/src/lib.rs::unique",
        ]
    );

    let ambiguous: Vec<bool> = report
        .functions
        .iter()
        .map(|f| f.identity_ambiguous)
        .collect();

    assert_eq!(ambiguous, [true, true, true, true, false]);
}

#[test]
fn tests_and_benches_directories_are_test_files() {
    let report = scenario("layout");
    let expected = [
        ExpectedFile {
            path: "src/lib.rs",
            category: Category::Production,
            status: FileStatus::Analyzed,
        },
        ExpectedFile {
            path: "tests/integration.rs",
            category: Category::Test,
            status: FileStatus::Analyzed,
        },
        ExpectedFile {
            path: "benches/speed.rs",
            category: Category::Test,
            status: FileStatus::Analyzed,
        },
        ExpectedFile {
            path: "src/made.rs",
            category: Category::Generated,
            status: FileStatus::Excluded,
        },
        ExpectedFile {
            path: "src/proto.rs",
            category: Category::Generated,
            status: FileStatus::Excluded,
        },
    ];

    for want in &expected {
        assert_file(&report, want);
    }

    let by_name: Vec<(&str, Category)> = report
        .functions
        .iter()
        .map(|f| (f.name.as_str(), f.category))
        .collect();

    assert_eq!(
        by_name,
        [
            ("speed", Category::Test),
            ("library", Category::Production),
            ("integration", Category::Test),
        ]
    );
    assert_eq!(report.metrics.test.functions, 2);
    assert_eq!(report.metrics.production.functions, 1);
}

#[test]
fn a_tests_directory_only_counts_for_rust_files() {
    let root = tree();

    write(
        root.path(),
        "tests/helper.go",
        "package tests\n\nfunc Helper() int { return 1 }\n",
    );
    write(
        root.path(),
        "tests/helper.ts",
        "export function helper() { return 1 }\n",
    );
    write(root.path(), "src/tests_like.rs", "pub fn real() {}\n");

    let report = audit_root(root.path()).report;

    assert_file(
        &report,
        &ExpectedFile {
            path: "tests/helper.go",
            category: Category::Production,
            status: FileStatus::Analyzed,
        },
    );
    assert_file(
        &report,
        &ExpectedFile {
            path: "tests/helper.ts",
            category: Category::Production,
            status: FileStatus::Analyzed,
        },
    );
    assert_file(
        &report,
        &ExpectedFile {
            path: "src/tests_like.rs",
            category: Category::Production,
            status: FileStatus::Analyzed,
        },
    );
}

#[test]
fn excluding_tests_leaves_rust_test_directories_out() {
    let mut settings = options();
    settings.include_tests = false;

    let outcome = audit(&corpus().join("layout"), &settings, &mut io::sink());
    let report = outcome.report;

    assert_file(
        &report,
        &ExpectedFile {
            path: "tests/integration.rs",
            category: Category::Test,
            status: FileStatus::Excluded,
        },
    );
    assert_eq!(names(&report), ["library"]);
}

#[test]
fn generated_headers_mark_rust_files() {
    let root = tree();

    write(root.path(), "plain.rs", "pub fn a() {}\n");
    write(root.path(), "at.rs", "// @generated\npub fn a() {}\n");
    write(
        root.path(),
        "at_by.rs",
        "//! Docs.\n\n// @generated by protoc\npub fn a() {}\n",
    );
    write(
        root.path(),
        "code.rs",
        "// Code generated by prost. DO NOT EDIT.\npub fn a() {}\n",
    );
    write(root.path(), "late.rs", "pub fn a() {}\n// @generated\n");
    write(root.path(), "glued.rs", "// @generatedfoo\npub fn a() {}\n");
    write(root.path(), "block.rs", "/* @generated */\npub fn a() {}\n");

    let report = audit_root(root.path()).report;
    let generated: Vec<&str> = report
        .files
        .iter()
        .filter(|file| file.category == Category::Generated)
        .map(|file| file.path.as_str())
        .collect();

    assert_eq!(generated, ["at.rs", "at_by.rs", "code.rs"]);
}

#[test]
fn cargo_output_directories_are_skipped_only_when_tagged() {
    let root = tree();

    write(root.path(), "src/lib.rs", "pub fn a() {}\n");
    write(
        root.path(),
        "target/CACHEDIR.TAG",
        "Signature: 8a477f597d28d172789f06886806bc55\n",
    );
    write(
        root.path(),
        "target/debug/build/gen.rs",
        "pub fn built() {}\n",
    );
    write(
        root.path(),
        "crates/inner/target/CACHEDIR.TAG",
        "Signature: x\n",
    );
    write(
        root.path(),
        "crates/inner/target/release/out.rs",
        "pub fn built() {}\n",
    );
    write(
        root.path(),
        "docs/target/example.rs",
        "pub fn example() {}\n",
    );
    write(
        root.path(),
        "notes/target/CACHEDIR.TAG/inside.rs",
        "pub fn weird() {}\n",
    );

    let report = audit_root(root.path()).report;
    let paths: Vec<&str> = report.files.iter().map(|file| file.path.as_str()).collect();

    assert_eq!(
        paths,
        [
            "docs/target/example.rs",
            "notes/target/CACHEDIR.TAG/inside.rs",
            "src/lib.rs",
        ]
    );
    assert_eq!(report.coverage.excluded_directories, 2);
}

#[test]
fn a_syntax_error_is_a_parse_error() {
    let root = tree();

    write(root.path(), "src/good.rs", "pub fn good() {}\n");
    write(
        root.path(),
        "src/bad.rs",
        "pub fn bad( {\n    let x = ;\n}\n",
    );

    let outcome = audit_root(root.path());
    let report = outcome.report;

    assert_file(
        &report,
        &ExpectedFile {
            path: "src/bad.rs",
            category: Category::Production,
            status: FileStatus::ParseError,
        },
    );
    assert_eq!(report.coverage.parse_errors, 1);
    assert_eq!(names(&report), ["good"]);
    assert_eq!(outcome.errors.len(), 1);

    let bad = report.files.iter().find(|file| file.path == "src/bad.rs");

    assert!(bad.is_some_and(|file| file.error.starts_with("syntax error at ")));
}

#[test]
fn macro_bodies_are_not_measured() {
    let root = tree();

    write(
        root.path(),
        "src/lib.rs",
        "macro_rules! twice {
    ($e:expr) => {
        fn generated(x: u32) -> u32 {
            if x > 1 { 1 } else { 2 }
        }
    };
}

pub fn calls() -> u32 {
    let n = dbg!(if true { 1 } else { 2 });
    n
}
",
    );

    let report = audit_root(root.path()).report;

    assert_eq!(names(&report), ["calls"]);
    assert_eq!(function(&report, "calls").cyclomatic, 1);
}

#[test]
fn audits_are_deterministic() {
    let first = serde_json::to_string(&scenario("constructs")).expect("encode first report");
    let second = serde_json::to_string(&scenario("constructs")).expect("encode second report");

    assert_eq!(first, second);
}
