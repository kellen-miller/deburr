//! Rust ownership and panic signals: clone-like calls, unwrap-like calls,
//! panic macros, and `unsafe` blocks, counted per function.
//!
//! Each scenario is a temporary tree with one `src/lib.rs` (or a Go or
//! TypeScript file, for the languages that must report zero).

use std::fs;
use std::io;
use std::path::Path;

use deburr::audit::{AuditOptions, audit};
use deburr::report::Report;

/// The four Rust signal counters of one function.
#[derive(Debug, PartialEq, Eq)]
struct Counts {
    clone_like: u32,
    unwrap_like: u32,
    panic_macros: u32,
    unsafe_blocks: u32,
}

/// One file of a scenario tree.
struct SourceFile {
    path: &'static str,
    contents: &'static str,
}

/// A function body and the counts it must produce.
struct Case {
    label: &'static str,
    source: &'static str,
    expected: Counts,
}

const fn counts(
    clone_like: u32,
    unwrap_like: u32,
    panic_macros: u32,
    unsafe_blocks: u32,
) -> Counts {
    Counts {
        clone_like,
        unwrap_like,
        panic_macros,
        unsafe_blocks,
    }
}

fn audit_files(files: &[SourceFile]) -> Report {
    let root = tempfile::tempdir().expect("create temp dir");

    for file in files {
        let path = root.path().join(file.path);

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent directories");
        }

        fs::write(path, file.contents).expect("write fixture");
    }

    audit_tree(root.path())
}

fn audit_tree(root: &Path) -> Report {
    let options = AuditOptions {
        include_tests: true,
        include_testdata: false,
        include_vendor: false,
        include_generated: false,
        max_file_bytes: None,
        duplication: None,
    };
    let outcome = audit(root, &options, &mut io::sink());

    assert!(outcome.errors.is_empty(), "{:?}", outcome.errors);
    assert_eq!(outcome.report.coverage.parse_errors, 0);

    outcome.report
}

fn counts_of(report: &Report, name: &str) -> Counts {
    let function = report
        .functions
        .iter()
        .find(|function| function.name == name)
        .unwrap_or_else(|| {
            let names: Vec<&str> = report.functions.iter().map(|f| f.name.as_str()).collect();

            panic!("function {name:?} not found among {names:?}")
        });
    let signals = &function.structural_signals;

    Counts {
        clone_like: signals.clone_like_calls,
        unwrap_like: signals.unwrap_like_calls,
        panic_macros: signals.panic_macros,
        unsafe_blocks: signals.unsafe_blocks,
    }
}

/// Audits each case as `fn f` in its own file and checks its counts.
fn assert_cases(cases: &[Case]) {
    for case in cases {
        let report = audit_files(&[SourceFile {
            path: "src/lib.rs",
            contents: case.source,
        }]);

        assert_eq!(counts_of(&report, "f"), case.expected, "{}", case.label);
    }
}

#[test]
fn clone_like_calls_are_method_calls_with_no_arguments() {
    assert_cases(&[
        Case {
            label: "the three clone-like methods",
            source: "fn f(a: &String, v: &[u8]) {\n    a.clone();\n    a.to_owned();\n    v.iter().cloned();\n}\n",
            expected: counts(3, 0, 0, 0),
        },
        Case {
            label: "chained calls count once each",
            source: "fn f(a: &String) -> String {\n    a.clone().clone()\n}\n",
            expected: counts(2, 0, 0, 0),
        },
        Case {
            label: "a call that takes an argument is another method",
            source: "fn f(a: &String, b: &String) {\n    a.clone(b);\n    a.to_owned(b);\n    a.cloned(b);\n}\n",
            expected: counts(0, 0, 0, 0),
        },
        Case {
            label: "a field read is not a call",
            source: "struct S { clone: u8 }\nfn f(s: &S) -> u8 {\n    s.clone\n}\n",
            expected: counts(0, 0, 0, 0),
        },
        Case {
            label: "a path call is not a method call",
            source: "fn f(a: &String) -> String {\n    Clone::clone(a)\n}\n",
            expected: counts(0, 0, 0, 0),
        },
        Case {
            label: "similar names are different methods",
            source: "fn f(a: &String) {\n    a.clone_from_slice();\n    a.to_owned_lossy();\n    a.cloned_all();\n}\n",
            expected: counts(0, 0, 0, 0),
        },
        Case {
            label: "a comment between the parentheses is not an argument",
            source: "fn f(a: &String) -> String {\n    a.clone(/* none */)\n}\n",
            expected: counts(1, 0, 0, 0),
        },
    ]);
}

#[test]
fn turbofish_method_calls_count_their_signals() {
    assert_cases(&[
        Case {
            label: "turbofish cloned and to_owned",
            source: "fn f(v: &[u8], a: &str) {\n    v.iter().cloned::<u8>();\n    a.to_owned::<String>();\n}\n",
            expected: counts(2, 0, 0, 0),
        },
        Case {
            label: "turbofish clone with spacing and a comment",
            source: "fn f(a: &String) {\n    a.clone :: <String>();\n    a.clone::</* t */ String>();\n}\n",
            expected: counts(2, 0, 0, 0),
        },
        Case {
            label: "turbofish unwrap and expect",
            source: "fn f(o: Option<u8>, r: Result<u8, u8>) {\n    o.unwrap::<u8>();\n    r.expect::<u8>(\"why\");\n}\n",
            expected: counts(0, 2, 0, 0),
        },
        Case {
            label: "turbofish keeps the argument-count check",
            source: "fn f(a: &String, o: Option<u8>) {\n    a.clone::<String>(a);\n    o.unwrap::<u8>(1);\n    o.expect::<u8>();\n}\n",
            expected: counts(0, 0, 0, 0),
        },
        Case {
            label: "a turbofish path call is not a method call",
            source: "fn f(v: &Vec<u8>) -> Vec<u8> {\n    Vec::<u8>::clone(v)\n}\n",
            expected: counts(0, 0, 0, 0),
        },
        Case {
            label: "a turbofish call of a plain function is not a method call",
            source: "fn f() -> u8 {\n    clone::<u8>()\n}\n",
            expected: counts(0, 0, 0, 0),
        },
    ]);
}

#[test]
fn unwrap_like_calls_need_the_right_argument_count() {
    assert_cases(&[
        Case {
            label: "four unwrap-like methods and a total alternative",
            source: "fn f(a: Option<u8>, b: Result<u8, u8>) {\n    a.unwrap();\n    b.unwrap_err();\n    b.expect(\"m\");\n    b.expect_err(\"m\");\n    a.unwrap_or(1);\n}\n",
            expected: counts(0, 4, 0, 0),
        },
        Case {
            label: "unwrap_unchecked is also an unsafe block",
            source: "fn f(a: Option<u8>) -> u8 {\n    unsafe { a.unwrap_unchecked() }\n}\n",
            expected: counts(0, 1, 0, 1),
        },
        Case {
            label: "unwrap takes none and expect takes one",
            source: "fn f(a: Option<u8>) {\n    a.unwrap(1);\n    a.expect();\n    a.expect(\"a\", \"b\");\n    a.expect_err();\n}\n",
            expected: counts(0, 0, 0, 0),
        },
        Case {
            label: "total alternatives are not unwrap-like",
            source: "fn f(a: Option<u8>) {\n    a.unwrap_or(1);\n    a.unwrap_or_default();\n    a.unwrap_or_else(|| 2);\n}\n",
            expected: counts(0, 0, 0, 0),
        },
    ]);
}

#[test]
fn panic_macros_match_the_last_path_segment() {
    assert_cases(&[
        Case {
            label: "the four panic macros in each delimiter",
            source: "fn f(x: u8) {\n    panic!(\"x\");\n    todo!();\n    unimplemented![];\n    unreachable!{};\n}\n",
            expected: counts(0, 0, 4, 0),
        },
        Case {
            label: "a scoped macro path",
            source: "fn f() {\n    std::panic!(\"x\");\n    core::unreachable!();\n}\n",
            expected: counts(0, 0, 2, 0),
        },
        Case {
            label: "other macros and functions do not count",
            source: "fn f() {\n    my_panic!();\n    panic_any(1);\n    assert!(true);\n    unreachable_unchecked();\n    todo_later!();\n}\n",
            expected: counts(0, 0, 0, 0),
        },
    ]);
}

#[test]
fn unsafe_blocks_count_once_each() {
    assert_cases(&[
        Case {
            label: "sibling and nested blocks",
            source: "fn f(p: *const u8) -> u8 {\n    let a = unsafe { *p };\n    unsafe {\n        let b = unsafe { *p };\n        a + b\n    }\n}\n",
            expected: counts(0, 0, 0, 3),
        },
        Case {
            label: "an unsafe fn is not a block",
            source: "unsafe fn f(p: *const u8) -> u8 {\n    *p\n}\n",
            expected: counts(0, 0, 0, 0),
        },
    ]);
}

#[test]
fn nested_functions_keep_their_own_signals() {
    let source = "\
fn outer(a: &String, o: Option<u8>) -> usize {
    let first = a.clone();
    let apply = |b: &String| {
        let second = b.clone();
        let third = o.unwrap();
        panic!(\"{}\", second);
    };
    fn inner(c: &String) -> String {
        unsafe { todo!() };
        c.to_owned()
    }
    apply(&first);
    first.len()
}
";
    let report = audit_files(&[SourceFile {
        path: "src/lib.rs",
        contents: source,
    }]);

    assert_eq!(counts_of(&report, "outer"), counts(1, 0, 0, 0));
    assert_eq!(counts_of(&report, "outer::{closure#1}"), counts(1, 1, 1, 0));
    assert_eq!(counts_of(&report, "outer::inner"), counts(1, 0, 1, 1));
}

#[test]
fn macro_arguments_are_not_measured() {
    // Known limitation: the grammar leaves a macro's arguments as a token
    // tree, so code inside one is invisible to every measurement.
    let source = "fn f(a: &String, o: Option<u8>) {\n    println!(\"{}\", a.clone());\n    vec![o.unwrap()];\n}\n";
    let report = audit_files(&[SourceFile {
        path: "src/lib.rs",
        contents: source,
    }]);

    assert_eq!(counts_of(&report, "f"), counts(0, 0, 0, 0));
}

#[test]
fn inline_test_functions_count_their_signals() {
    let source = "\
fn production(o: Option<u8>) -> u8 {
    o.unwrap()
}

#[cfg(test)]
mod tests {
    #[test]
    fn checks() {
        let o = Some(1);
        assert_eq!(o.unwrap(), 1);
        let a = String::new();
        a.clone();
        panic!(\"boom\");
    }
}
";
    let report = audit_files(&[SourceFile {
        path: "src/lib.rs",
        contents: source,
    }]);

    assert_eq!(counts_of(&report, "production"), counts(0, 1, 0, 0));
    assert_eq!(counts_of(&report, "tests::checks"), counts(1, 0, 1, 0));
}

#[test]
fn go_and_typescript_report_zero() {
    let go = "package main\n\nfunc F(a *T) {\n\ta.Clone()\n\ta.Unwrap()\n\tpanic(\"x\")\n}\n";
    let typescript =
        "export function f(a: any) {\n  a.clone();\n  a.unwrap();\n  throw new Error('x');\n}\n";
    let report = audit_files(&[
        SourceFile {
            path: "main.go",
            contents: go,
        },
        SourceFile {
            path: "main.ts",
            contents: typescript,
        },
    ]);

    assert_eq!(report.functions.len(), 2);

    for function in &report.functions {
        let signals = &function.structural_signals;

        assert_eq!(
            [
                signals.clone_like_calls,
                signals.unwrap_like_calls,
                signals.panic_macros,
                signals.unsafe_blocks,
            ],
            [0; 4],
            "{} signals",
            function.name
        );
    }
}
