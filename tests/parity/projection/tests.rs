use serde_json::json;

use super::*;

fn report(code_lines: i64, status: &str) -> Value {
    json!({
        "files": [
            {"path": "a.go", "category": "production", "status": status, "lines": 10, "code_lines": code_lines},
            {"path": "a.go", "category": "test", "status": "analyzed", "lines": 1, "code_lines": 1},
        ],
        "coverage": {"discovered": 2, "by_category": []},
        "metrics": {"all": {"code_lines": code_lines}},
    })
}

/// A Go file plus a Rust file and function, as the Rust analyzer reports
/// them; the Go version reports the same Rust file as unsupported.
fn mixed_report() -> Value {
    let function = |path: &str, name: &str, cyclomatic: i64, nesting: i64, high: bool| {
        json!({
            "path": path, "name": name, "category": "production", "sloc": 3,
            "cyclomatic": cyclomatic, "max_nesting": nesting, "mass": cyclomatic as f64,
            "high_complexity": high,
        })
    };
    let row = |category: &str, discovered: i64, analyzed: i64, unsupported: i64| {
        json!({
            "category": category, "discovered": discovered, "analyzed": analyzed,
            "excluded": 0, "unsupported": unsupported, "read_errors": 0, "parse_errors": 0,
        })
    };

    json!({
        "files": [
            {"path": "a.go", "category": "production", "status": "analyzed", "lines": 4, "code_lines": 3},
            {"path": "src/b.rs", "category": "production", "status": "analyzed", "lines": 9, "code_lines": 7},
            {"path": "src/c.rs", "category": "unsupported", "status": "unsupported", "lines": 1, "code_lines": 0},
        ],
        "functions": [
            function("a.go", "A", 2, 1, false),
            function("src/b.rs", "b", 11, 4, true),
        ],
        "findings": [
            {"path": "src/b.rs", "rule": "duplicate-branches", "start": {}, "end": {}},
        ],
        "coverage": {
            "discovered": 3, "analyzed": 2, "excluded": 0, "unsupported": 1,
            "read_errors": 0, "parse_errors": 0,
            "by_category": [row("production", 2, 2, 0), row("unsupported", 1, 0, 1)],
        },
        "metrics": {
            "all": {"code_lines": 10, "functions": 2, "cyclomatic": 13, "max_nesting": 4,
                "mass": 13.0, "high_complexity_functions": 1, "high_complexity_mass": 11.0,
                "high_complexity_mass_share": 0.846_153_846_153_846_2},
            "production": {"code_lines": 10, "functions": 2, "cyclomatic": 13, "max_nesting": 4,
                "mass": 13.0, "high_complexity_functions": 1, "high_complexity_mass": 11.0,
                "high_complexity_mass_share": 0.846_153_846_153_846_2},
            "test": {"code_lines": 0, "functions": 0, "cyclomatic": 0, "max_nesting": 0,
                "mass": 0.0, "high_complexity_functions": 0, "high_complexity_mass": 0.0,
                "high_complexity_mass_share": null},
        },
    })
}

#[test]
fn inline_test_lines_leave_the_test_bucket() {
    // `src/b.rs` holds 7 code lines, 2 of them inline tests: the Rust
    // analyzer counted 5 in production and 2 in test, so removing the
    // file takes 5 from production and 2 from test, leaving the Go file.
    let mut report = mixed_report();
    report["files"][1]["inline_test_code_lines"] = json!(2);
    report["metrics"]["production"]["code_lines"] = json!(8);
    report["metrics"]["test"]["code_lines"] = json!(2);

    let filtered = without_rust(&report).expect("report has Rust files");

    assert_eq!(filtered["metrics"]["all"]["code_lines"], json!(3));
    assert_eq!(filtered["metrics"]["production"]["code_lines"], json!(3));
    assert_eq!(filtered["metrics"]["test"]["code_lines"], json!(0));
}

/// The report the Go version produces for the Go file of
/// [`mixed_report`].
fn go_only_report() -> Value {
    json!({
        "files": [{"path": "a.go", "category": "production", "status": "analyzed", "lines": 4, "code_lines": 3}],
        "functions": [{
            "path": "a.go", "name": "A", "category": "production", "sloc": 3, "cyclomatic": 2,
            "max_nesting": 1, "mass": 2.0, "high_complexity": false,
        }],
        "coverage": {
            "discovered": 1, "analyzed": 1, "excluded": 0, "unsupported": 0,
            "read_errors": 0, "parse_errors": 0,
            "by_category": [
                {"category": "production", "discovered": 1, "analyzed": 1, "excluded": 0,
                 "unsupported": 0, "read_errors": 0, "parse_errors": 0},
                {"category": "unsupported", "discovered": 0, "analyzed": 0, "excluded": 0,
                 "unsupported": 0, "read_errors": 0, "parse_errors": 0},
            ],
        },
        "metrics": {
            "all": {"code_lines": 3, "functions": 1, "cyclomatic": 2, "max_nesting": 1,
                "mass": 2.0, "high_complexity_functions": 0, "high_complexity_mass": 0.0,
                "high_complexity_mass_share": 0.0},
            "production": {"code_lines": 3, "functions": 1, "cyclomatic": 2, "max_nesting": 1,
                "mass": 2.0, "high_complexity_functions": 0, "high_complexity_mass": 0.0,
                "high_complexity_mass_share": 0.0},
            "test": {"code_lines": 0, "functions": 0, "cyclomatic": 0, "max_nesting": 0,
                "mass": 0.0, "high_complexity_functions": 0, "high_complexity_mass": 0.0,
                "high_complexity_mass_share": null},
        },
    })
}

#[test]
fn rust_files_and_their_totals_are_left_out() {
    let projection = project(&mixed_report());

    assert_eq!(projection.len("files"), 1);
    assert_eq!(projection.len("functions"), 1);
    assert_eq!(projection.len("findings"), 0);
    assert!(diff(&project(&go_only_report()), &projection).is_empty());
}

/// One wrong aggregate figure in the mixed report.
struct Corruption {
    bucket: &'static str,
    field: &'static str,
    value: Value,
}

#[test]
fn corrupted_aggregates_survive_the_projection() {
    let corruptions = [
        Corruption {
            bucket: "all",
            field: "functions",
            value: json!(3),
        },
        Corruption {
            bucket: "production",
            field: "cyclomatic",
            value: json!(14),
        },
        Corruption {
            bucket: "all",
            field: "mass",
            value: json!(14.0),
        },
        Corruption {
            bucket: "production",
            field: "high_complexity_functions",
            value: json!(2),
        },
        Corruption {
            bucket: "all",
            field: "high_complexity_mass",
            value: json!(12.0),
        },
        // Above every function's nesting.
        Corruption {
            bucket: "all",
            field: "max_nesting",
            value: json!(6),
        },
        // Below the removed function's own nesting.
        Corruption {
            bucket: "all",
            field: "max_nesting",
            value: json!(2),
        },
        // Inconsistent with the reported masses (11 / 13).
        Corruption {
            bucket: "production",
            field: "high_complexity_mass_share",
            value: json!(0.5),
        },
        // The test bucket holds no Rust function, so it is kept as reported.
        Corruption {
            bucket: "test",
            field: "functions",
            value: json!(1),
        },
    ];
    let go = project(&go_only_report());

    for corruption in corruptions {
        let mut report = mixed_report();
        report["metrics"][corruption.bucket][corruption.field] = corruption.value;

        let differences = diff(&go, &project(&report));

        assert!(
            differences
                .iter()
                .any(|difference| difference.section == "metrics"
                    && difference.key == format!("{}#1", corruption.bucket)
                    && difference.field == corruption.field),
            "{}.{} corruption vanished: {differences:?}",
            corruption.bucket,
            corruption.field
        );
    }
}

#[test]
fn underreported_nesting_equal_to_the_rust_maximum_survives() {
    // The Go function nests 5 deep, but the report claims the maximum is
    // the Rust function's 4.
    let mut report = mixed_report();
    report["functions"][0]["max_nesting"] = json!(5);

    let projected = project(&report);
    let mut go = go_only_report();
    go["functions"][0]["max_nesting"] = json!(5);
    go["metrics"]["all"]["max_nesting"] = json!(5);
    go["metrics"]["production"]["max_nesting"] = json!(5);

    let differences = diff(&project(&go), &projected);

    assert!(
        differences
            .iter()
            .any(|difference| difference.field == "max_nesting"),
        "underreported nesting vanished: {differences:?}"
    );
}

#[test]
fn rust_inline_test_functions_leave_the_test_bucket_totals() {
    let mut report = mixed_report();
    let test_function = json!({
        "path": "src/b.rs", "name": "t", "category": "test", "sloc": 2,
        "cyclomatic": 3, "max_nesting": 1, "mass": 3.0, "high_complexity": false,
    });

    report["functions"]
        .as_array_mut()
        .expect("functions array")
        .push(test_function);
    report["metrics"]["all"]["functions"] = json!(3);
    report["metrics"]["all"]["cyclomatic"] = json!(16);
    report["metrics"]["all"]["mass"] = json!(16.0);
    report["metrics"]["test"]["functions"] = json!(1);
    report["metrics"]["test"]["cyclomatic"] = json!(3);
    report["metrics"]["test"]["max_nesting"] = json!(1);
    report["metrics"]["test"]["mass"] = json!(3.0);
    report["metrics"]["all"]["high_complexity_mass_share"] = json!(11.0 / 16.0);
    report["metrics"]["test"]["high_complexity_mass_share"] = json!(0.0);

    let go = project(&go_only_report());

    assert!(diff(&go, &project(&report)).is_empty());

    report["metrics"]["test"]["cyclomatic"] = json!(4);

    assert!(!diff(&go, &project(&report)).is_empty());
}

#[test]
fn reports_without_rust_files_project_unchanged() {
    assert!(without_rust(&report(5, "analyzed")).is_none());
}

#[test]
fn repeated_keys_are_compared_one_to_one() {
    let projection = project(&report(5, "analyzed"));

    assert_eq!(projection.len("files"), 2);
    assert!(projection.sections["files"].contains_key("a.go#1"));
    assert!(projection.sections["files"].contains_key("a.go#2"));
}

#[test]
fn diff_names_the_differing_field() {
    let go = project(&report(5, "analyzed"));
    let rust = project(&report(4, "parse_error"));
    let differences = diff(&go, &rust);

    assert!(
        differences
            .iter()
            .any(|d| d.key == "a.go#1" && d.field == "code_lines" && d.go == "5" && d.rust == "4")
    );
    assert!(
        differences
            .iter()
            .any(|d| d.key == "a.go#1" && d.field == "status")
    );
    assert!(diff(&go, &go).is_empty());
}

#[test]
fn findings_project_range_owner_and_occurrence() {
    let finding = |line: i64, owner: &str| {
        json!({
            "path": "a.go",
            "rule": "duplicate-branches",
            "start": {"line": line, "column": 2},
            "end": {"line": line + 4, "column": 2},
            "details": [
                {"key": "owner", "value": owner},
                {"key": "candidate_only", "value": "true"},
            ],
        })
    };
    let go = project(&json!({"findings": [finding(3, "F"), finding(9, "G")]}));
    let rust = project(&json!({"findings": [finding(3, "F"), finding(10, "G")]}));
    let differences = diff(&go, &rust);

    assert_eq!(go.len("findings"), 2);
    assert!(go.sections["findings"].contains_key("a.go::duplicate-branches#2"));
    assert!(differences.iter().all(|d| d.section == "findings"));
    assert!(
        differences
            .iter()
            .any(|d| d.key == "a.go::duplicate-branches#2" && d.field == "start_line")
    );
    assert!(!differences.iter().any(|d| d.field == "owner"));
}

#[test]
fn anonymous_digests_are_masked_wherever_they_appear() {
    let digest = "ab".repeat(32);

    assert_eq!(mask_anonymous_digests(&format!("anon-{digest}")), "anon-*");
    assert_eq!(
        mask_anonymous_digests(&format!("outer.anon-{digest}.anon-{digest}")),
        "outer.anon-*.anon-*"
    );
    // Shorter hex runs are ordinary names, as is `anon-` with no digits.
    assert_eq!(mask_anonymous_digests("anon-abc"), "anon-abc");
    assert_eq!(mask_anonymous_digests("anon-"), "anon-");
    assert_eq!(mask_anonymous_digests("outer."), "outer.");
}

#[test]
fn stored_projection_round_trips() {
    let original = project(&report(5, "analyzed"));
    let stored = original.to_json();
    let restored = Projection::from_json(&stored).expect("decode stored projection");

    assert!(diff(&original, &restored).is_empty());
    assert_eq!(restored.len("files"), 2);
    assert_eq!(restored.to_json(), stored);
    assert!(Projection::from_json(&json!({"bogus": {}})).is_err());
    assert!(Projection::from_json(&json!({"files": []})).is_err());
}

#[test]
fn missing_entries_are_reported() {
    let go = project(&report(5, "analyzed"));
    let mut rust = project(&report(5, "analyzed"));
    rust.sections
        .get_mut("files")
        .map(|files| files.remove("a.go#2"));

    let differences = diff(&go, &rust);

    assert_eq!(differences.len(), 1);
    assert_eq!(differences[0].field, "<entry>");
    assert_eq!(differences[0].rust, "missing");
}

#[test]
fn floats_compare_with_tolerance() {
    assert!(Scalar::Float(1.0).equivalent(&Scalar::Float(1.0 + 1e-12)));
    assert!(Scalar::Int(8).equivalent(&Scalar::Float(8.0)));
    assert!(!Scalar::Float(1.0).equivalent(&Scalar::Float(1.1)));
}

#[test]
fn accepted_entries_cover_exactly_one_field() {
    let accepted = Accepted {
        section: "files".into(),
        key: "a.go#1".into(),
        field: "code_lines".into(),
        reason: "test".into(),
    };
    let go = project(&report(5, "analyzed"));
    let rust = project(&report(4, "parse_error"));
    let differences = diff(&go, &rust);

    let covered: Vec<&Difference> = differences.iter().filter(|d| accepted.covers(d)).collect();

    assert_eq!(covered.len(), 1);
    assert!(differences.len() > 1);
}
