//! Which functions a clone endpoint names: body overlap, neighbors, imports.

use deburr::report::{Category, DuplicationStatus, Report};

use super::{
    assert_functions_resolve, endpoint_paths, go_function, run, run_default, settings, write,
};

#[test]
fn an_identical_function_is_one_clone_naming_both_functions() {
    let dir = tempfile::tempdir().expect("temp dir");
    let function = go_function("Alpha", 20);
    write(dir.path(), "a.go", &format!("package a\n\n{function}"));
    write(dir.path(), "b.go", &format!("package b\n\n{function}"));

    let report = run_default(dir.path());

    assert_eq!(report.duplication.status, DuplicationStatus::Measured);
    assert_eq!(report.duplication.clones.len(), 1);

    let clone = &report.duplication.clones[0];
    let mut named: Vec<&str> = clone
        .endpoints
        .iter()
        .flat_map(|endpoint| endpoint.functions.iter().map(String::as_str))
        .collect();
    named.sort_unstable();

    assert_eq!(clone.category, Category::Production);
    assert_eq!(endpoint_paths(clone), ["a.go", "b.go"]);
    assert_eq!(named, ["a/a.go::Alpha", "b/b.go::Alpha"]);
    assert!(
        clone
            .endpoints
            .iter()
            .all(|endpoint| endpoint.code_lines == 24)
    );
    assert!(!clone.identity_ambiguous);
    assert_functions_resolve(&report);
}

#[test]
fn a_run_spanning_two_functions_names_both() {
    let dir = tempfile::tempdir().expect("temp dir");
    let both = format!("{}\n{}", go_function("One", 10), go_function("Two", 10));
    write(dir.path(), "a.go", &format!("package a\n\n{both}"));
    write(dir.path(), "b.go", &format!("package b\n\n{both}"));

    let report = run_default(dir.path());

    assert_eq!(report.duplication.clones.len(), 1);

    for endpoint in &report.duplication.clones[0].endpoints {
        let package = &endpoint.path[..1];

        assert_eq!(
            endpoint.functions,
            [
                format!("{package}/{}::One", endpoint.path),
                format!("{package}/{}::Two", endpoint.path)
            ]
        );
    }

    assert_functions_resolve(&report);
}

/// A TypeScript function of `statements + 4` lines.
fn ts_function(name: &str, statements: usize) -> String {
    let body: String = (1..=statements)
        .map(|index| format!("  total += x * {index};\n"))
        .collect();

    format!("function {name}(x: number): number {{\n  let total = 0;\n{body}  return total;\n}}\n")
}

/// The report's only clone ends on the keyword that opens the neighbor
/// declaration, and each endpoint names just the function it copies.
fn assert_clone_names_only(report: &Report, end_line: u32, owners: [&str; 2]) {
    assert_eq!(report.duplication.clones.len(), 1);

    for (endpoint, owner) in report.duplication.clones[0].endpoints.iter().zip(owners) {
        assert_eq!(endpoint.end.line, end_line, "{}", endpoint.path);
        assert_eq!(endpoint.functions, [owner], "{}", endpoint.path);
    }

    assert_functions_resolve(report);
}

#[test]
fn a_go_run_ending_at_the_next_func_keyword_does_not_name_that_function() {
    let dir = tempfile::tempdir().expect("temp dir");
    let alpha = go_function("Alpha", 20);
    write(
        dir.path(),
        "a.go",
        &format!("package a\n\n{alpha}\nfunc Next() {{\n}}\n"),
    );
    write(
        dir.path(),
        "b.go",
        &format!("package b\n\n{alpha}\nfunc Other() {{\n}}\n"),
    );

    let report = run_default(dir.path());

    assert_clone_names_only(&report, 28, ["a/a.go::Alpha", "b/b.go::Alpha"]);
}

#[test]
fn a_typescript_run_ending_at_the_next_function_keyword_does_not_name_it() {
    let dir = tempfile::tempdir().expect("temp dir");
    let alpha = ts_function("alpha", 20);
    let (next, other) = ("function next() {\n}\n", "function other() {\n}\n");

    write(dir.path(), "one.ts", &format!("{alpha}\n{next}"));
    write(dir.path(), "two.ts", &format!("{alpha}\n{other}"));

    let report = run_default(dir.path());

    assert_clone_names_only(
        &report,
        26,
        ["typescript/one.ts::alpha", "typescript/two.ts::alpha"],
    );
}

#[test]
fn a_run_inside_part_of_a_one_line_body_names_that_function() {
    // The shared run is inside the body of a one-line function whose
    // signature differs between the copies, so only body tokens overlap.
    let dir = tempfile::tempdir().expect("temp dir");
    let body = "const a = x + 1; const b = a * 2; const c = b - 3; return [a, b, c, a + b + c];";

    write(
        dir.path(),
        "one.ts",
        &format!("export function first(x: number) {{ {body} }}\n"),
    );
    write(
        dir.path(),
        "two.ts",
        &format!("export function second(x: number, unused: string) {{ {body} }}\n"),
    );

    let report = run(dir.path(), settings(8, 1));

    assert!(!report.duplication.clones.is_empty());

    for clone in &report.duplication.clones {
        let owners: Vec<&str> = clone
            .endpoints
            .iter()
            .flat_map(|endpoint| endpoint.functions.iter().map(String::as_str))
            .collect();

        assert_eq!(
            owners,
            ["typescript/one.ts::first", "typescript/two.ts::second"],
            "{clone:?}"
        );
    }

    assert_functions_resolve(&report);
}

#[test]
fn a_multiline_signature_alone_does_not_make_an_owner() {
    // The copies share a long multiline parameter list and nothing of either
    // body, so the clone covers only signature tokens and names no function.
    let dir = tempfile::tempdir().expect("temp dir");
    let parameters: String = (1..=12)
        .map(|index| format!("\tvalue{index} int,\n"))
        .collect();

    write(
        dir.path(),
        "a.go",
        &format!("package a\n\nfunc Alpha(\n{parameters}) int {{\n\treturn 1\n}}\n"),
    );
    write(
        dir.path(),
        "b.go",
        &format!("package b\n\nfunc Beta(\n{parameters}) string {{\n\treturn \"two\"\n}}\n"),
    );

    let report = run(dir.path(), settings(20, 1));

    assert!(!report.duplication.clones.is_empty());

    for clone in &report.duplication.clones {
        for endpoint in &clone.endpoints {
            assert!(endpoint.functions.is_empty(), "{endpoint:?}");
        }
    }
}

#[test]
fn a_run_covering_whole_one_line_functions_names_each_of_them() {
    let dir = tempfile::tempdir().expect("temp dir");
    let functions: String = (1..=10)
        .map(|index| format!("function f{index}(x: number) {{ return x + {index}; }}\n"))
        .collect();

    write(dir.path(), "one.ts", &format!("{functions}const a = 1;\n"));
    write(dir.path(), "two.ts", &format!("{functions}let b = 2;\n"));

    let report = run_default(dir.path());

    assert_eq!(report.duplication.clones.len(), 1);

    for endpoint in &report.duplication.clones[0].endpoints {
        for (index, id) in endpoint.functions.iter().enumerate() {
            assert!(id.ends_with(&format!("::f{}", index + 1)), "{id}");
        }

        assert_eq!(endpoint.functions.len(), 10, "{}", endpoint.path);
    }

    assert_functions_resolve(&report);
}

#[test]
fn a_run_of_only_imports_names_no_function() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut imports = String::new();

    for index in 1..=12 {
        imports.push_str(&format!(
            "import {{ name{index} }} from \"./module{index}\";\n"
        ));
    }

    write(
        dir.path(),
        "one.ts",
        &format!("{imports}const first = 1;\n"),
    );
    write(dir.path(), "two.ts", &format!("{imports}let second = 2;\n"));

    let report = run_default(dir.path());

    assert_eq!(report.duplication.clones.len(), 1);

    for endpoint in &report.duplication.clones[0].endpoints {
        assert_eq!((endpoint.start.line, endpoint.end.line), (1, 12));
        assert!(endpoint.functions.is_empty(), "{:?}", endpoint.functions);
    }

    assert_functions_resolve(&report);
}
