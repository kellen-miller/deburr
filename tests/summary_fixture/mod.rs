//! Shared fixtures for the summary test files.

use deburr::render::summary::{Summary, WorkItem};
use deburr::report::{Category, CodeClone, Function, Report};
use serde_json::json;

/// A clone endpoint: a region of `path` and the functions it overlaps.
pub(crate) struct Region<'a> {
    pub(crate) path: &'a str,
    pub(crate) start: u32,
    pub(crate) end: u32,
    pub(crate) functions: &'a [&'a str],
}

pub(crate) fn base_report() -> Report {
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
pub(crate) fn function(path: &str, name: &str, line: u32, sloc: u32, cyclomatic: u32) -> Function {
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

pub(crate) fn clone(id: &str, category: Category, regions: &[Region<'_>; 2]) -> CodeClone {
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

pub(crate) fn item<'a>(summary: &'a Summary, short_title: &str) -> &'a WorkItem {
    summary
        .worklist
        .iter()
        .find(|item| item.short_title == short_title)
        .unwrap_or_else(|| panic!("no worklist item {short_title:?}: {:?}", summary.worklist))
}
