//! Rust function names: `mod`, `impl`, and `trait` containers, generic
//! arguments, repeated names, and layout independence. There is no reference
//! implementation for Rust, so these assertions are the specification.

use std::fs;
use std::io;
use std::path::Path;

use deburr::audit::{AuditOptions, AuditOutcome, audit};
use deburr::report::Report;

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

fn names(report: &Report) -> Vec<&str> {
    report.functions.iter().map(|f| f.name.as_str()).collect()
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
fn impl_names_keep_type_structure_and_drop_only_generic_arguments() {
    let root = tree();

    write(
        root.path(),
        "src/lib.rs",
        "impl<'a> Meshgrid for (&'a ArrayRef1<f32>, &'a ArrayRef1<f32>) { fn meshgrid(&self) {} }
impl<'a> Meshgrid for (&'a ArrayRef1<f32>, &'a ArrayRef1<f32>, &'a ArrayRef1<f32>) { fn meshgrid(&self) {} }
impl<T> Take<T> for [u8; 4] { fn take(&self) {} }
impl<T> Take<T> for [u8; 8] { fn take(&self) {} }
impl<'a, T> View for &'a mut [T] { fn view(&self) {} }
impl<T> View for *const T { fn view(&self) {} }
impl <A as Base>::Output { fn out(&self) {} }
impl <A as Base>::Other<A> { fn other(&self) {} }
impl a::Tr<X> for a::b::Ty<Y> { fn go(&self) {} }
impl Tr for a  ::  b::Spaced<Y> { fn spaced(&self) {} }
impl Plain { fn plain(&self) {} }
impl Show for Plain { fn show(&self) {} }
",
    );

    let report = audit_root(root.path()).report;

    assert_eq!(
        names(&report),
        [
            "<(&'a ArrayRef1, &'a ArrayRef1) as Meshgrid>::meshgrid",
            "<(&'a ArrayRef1, &'a ArrayRef1, &'a ArrayRef1) as Meshgrid>::meshgrid",
            "<[u8; 4] as Take>::take",
            "<[u8; 8] as Take>::take",
            "<&'a mut [T] as View>::view",
            "<*const T as View>::view",
            "<A as Base>::Output::out",
            "<A as Base>::Other::other",
            "<a::b::Ty as a::Tr>::go",
            "<a::b::Spaced as Tr>::spaced",
            "Plain::plain",
            "<Plain as Show>::show",
        ]
    );
    assert!(report.functions.iter().all(|f| !f.id.contains('#')));
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
fn impl_names_do_not_depend_on_layout() {
    // rustfmt lays a long tuple type over several lines with a trailing
    // comma; the name must match the single-line spelling so reformatting
    // never changes a function's identity. A one-element tuple keeps its
    // comma because `(A,)` and `(A)` are different types.
    let single = tree();
    write(
        single.path(),
        "src/lib.rs",
        "impl Shape for (u8, u16) { fn area(&self) {} }\nimpl Shape for (u8,) { fn side(&self) {} }\n",
    );
    let multi = tree();
    write(
        multi.path(),
        "src/lib.rs",
        "impl Shape\n    for (\n        u8,\n        u16,\n    )\n{\n    fn area(&self) {}\n}\nimpl Shape for (u8,) {\n    fn side(&self) {}\n}\n",
    );

    let single = audit_root(single.path()).report;
    let multi = audit_root(multi.path()).report;

    assert_eq!(names(&single), names(&multi));
    assert_eq!(
        names(&single),
        ["<(u8, u16) as Shape>::area", "<(u8,) as Shape>::side"]
    );
}

#[test]
fn impl_names_ignore_spacing_and_optional_commas() {
    // Each pair spells one type two ways rustfmt can leave or produce.
    let loose = tree();
    write(
        loose.path(),
        "src/lib.rs",
        "impl A for (u8,u16) { fn a(&self) {} }\nimpl A for [u8;4] { fn b(&self) {} }\nimpl A for &(dyn Send+Sync) { fn c(&self) {} }\nimpl A for fn(u8,) { fn d(&self) {} }\nimpl A for &dyn Fn(u8,) { fn e(&self) {} }\n",
    );
    let tidy = tree();
    write(
        tidy.path(),
        "src/lib.rs",
        "impl A for (u8, u16) { fn a(&self) {} }\nimpl A for [u8; 4] { fn b(&self) {} }\nimpl A for &(dyn Send + Sync) { fn c(&self) {} }\nimpl A for fn(u8) { fn d(&self) {} }\nimpl A for &dyn Fn(u8) { fn e(&self) {} }\n",
    );

    let loose = audit_root(loose.path()).report;
    let tidy = audit_root(tidy.path()).report;

    assert_eq!(names(&loose), names(&tidy));
    assert_eq!(
        names(&tidy),
        [
            "<(u8, u16) as A>::a",
            "<[u8; 4] as A>::b",
            "<&(dyn Send + Sync) as A>::c",
            "<fn(u8) as A>::d",
            "<&dyn Fn(u8) as A>::e",
        ]
    );
}

#[test]
fn one_element_tuples_keep_their_comma_inside_nested_lists() {
    // The commas inside the binder, the parameters, and the const block are
    // not the tuple's, so each tuple keeps its own and stays distinct from
    // the parenthesized type.
    let root = tree();
    write(
        root.path(),
        "src/lib.rs",
        "impl A for (for<'a, 'b> fn(&'a u8, &'b u8),) { fn a(&self) {} }\nimpl A for (for<'a, 'b> fn(&'a u8, &'b u8)) { fn b(&self) {} }\nimpl A for ([u8; { let x = [1, 2]; x[0] }],) { fn c(&self) {} }\n",
    );

    let report = audit_root(root.path()).report;

    assert_eq!(
        names(&report),
        [
            "<(for<'a, 'b> fn(&'a u8, &'b u8),) as A>::a",
            "<(for<'a, 'b> fn(&'a u8, &'b u8)) as A>::b",
            "<([u8; {let x = [1, 2]; x[0]}],) as A>::c",
        ]
    );
}
