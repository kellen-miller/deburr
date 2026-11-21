//! The semantic projection both reports are reduced to before diffing.
//!
//! A projection is a set of named sections; each section maps an entry key
//! to a flat set of scalar fields. Later milestones add sections (functions,
//! findings, per-bucket function metrics) by extending [`project`] and
//! calling [`Projection::insert`]; diffing and accepted-divergence handling
//! are generic over sections, entries, and fields.
//!
//! Entry keys carry an occurrence index (`key#1`, `key#2`, ...) assigned in
//! report order, so repeated keys (several Go `init` functions in one file,
//! say) are compared one-to-one and never collapsed.

use std::collections::{BTreeMap, HashMap};
use std::fmt;

use serde::Deserialize;
use serde_json::Value;

const FLOAT_TOLERANCE: f64 = 1e-9;

/// Every section [`project`] fills.
const SECTIONS: &[&str] = &["files", "coverage", "metrics", "functions", "findings"];

/// The structural signals compared per function.
const FUNCTION_SIGNALS: &[&str] = &[
    "flat_guards",
    "field_mappings",
    "nested_branches",
    "assertion_like_calls",
];

/// One projected value.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Scalar {
    Int(i64),
    Float(f64),
    Text(String),
    Bool(bool),
    Null,
}

impl Scalar {
    fn from_json(value: Option<&Value>) -> Self {
        match value {
            Some(Value::Bool(flag)) => Self::Bool(*flag),
            Some(Value::String(text)) => Self::Text(text.clone()),
            Some(Value::Number(number)) => match number.as_i64() {
                Some(integer) => Self::Int(integer),
                None => Self::Float(number.as_f64().unwrap_or(f64::NAN)),
            },
            _ => Self::Null,
        }
    }

    fn to_json(&self) -> Value {
        match self {
            Self::Int(value) => Value::from(*value),
            Self::Float(value) => Value::from(*value),
            Self::Text(value) => Value::from(value.as_str()),
            Self::Bool(value) => Value::from(*value),
            Self::Null => Value::Null,
        }
    }

    fn equivalent(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Float(left), Self::Float(right)) => (left - right).abs() <= FLOAT_TOLERANCE,
            // A float that happens to be integral may decode as an integer.
            (Self::Int(left), Self::Float(right)) | (Self::Float(right), Self::Int(left)) => {
                (*left as f64 - right).abs() <= FLOAT_TOLERANCE
            }
            _ => self == other,
        }
    }
}

impl fmt::Display for Scalar {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(value) => write!(formatter, "{value}"),
            Self::Float(value) => write!(formatter, "{value}"),
            Self::Text(value) => write!(formatter, "{value}"),
            Self::Bool(value) => write!(formatter, "{value}"),
            Self::Null => write!(formatter, "null"),
        }
    }
}

/// The fields of one entry, by name.
pub(crate) type Fields = BTreeMap<String, Scalar>;

/// A report reduced to the facts the parity bar compares.
#[derive(Debug, Default)]
pub(crate) struct Projection {
    sections: BTreeMap<&'static str, BTreeMap<String, Fields>>,
    occurrences: HashMap<(&'static str, String), u32>,
}

impl Projection {
    /// Adds an entry under `section`, keyed by `key` plus its occurrence
    /// index among entries that share `key`.
    pub(crate) fn insert(&mut self, section: &'static str, key: &str, fields: Fields) {
        let count = self
            .occurrences
            .entry((section, key.to_owned()))
            .or_insert(0);
        *count += 1;

        self.sections
            .entry(section)
            .or_default()
            .insert(format!("{key}#{count}"), fields);
    }

    /// Number of entries in `section`.
    pub(crate) fn len(&self, section: &str) -> usize {
        self.sections.get(section).map_or(0, BTreeMap::len)
    }

    /// The stored form of a projection: `{section: {key: {field: scalar}}}`,
    /// where every key already carries its occurrence index.
    pub(crate) fn to_json(&self) -> Value {
        let sections = self.sections.iter().map(|(section, entries)| {
            let entries = entries.iter().map(|(key, fields)| {
                let fields = fields
                    .iter()
                    .map(|(name, scalar)| (name.clone(), scalar.to_json()))
                    .collect::<serde_json::Map<String, Value>>();

                (key.clone(), Value::Object(fields))
            });

            ((*section).to_owned(), Value::Object(entries.collect()))
        });

        Value::Object(sections.collect())
    }

    /// Reads a projection written by [`Projection::to_json`]. Sections are
    /// restricted to the names [`project`] produces so they stay `'static`.
    pub(crate) fn from_json(stored: &Value) -> Result<Self, String> {
        let stored = stored
            .as_object()
            .ok_or_else(|| "stored projection is not an object".to_owned())?;
        let mut projection = Self::default();

        for (name, entries) in stored {
            let section = SECTIONS
                .iter()
                .find(|known| **known == name)
                .ok_or_else(|| format!("unknown section {name}"))?;
            let entries = entries
                .as_object()
                .ok_or_else(|| format!("section {name} is not an object"))?;
            let target = projection.sections.entry(section).or_default();

            for (key, fields) in entries {
                let fields = fields
                    .as_object()
                    .ok_or_else(|| format!("entry {name}/{key} is not an object"))?;

                target.insert(
                    key.clone(),
                    fields
                        .iter()
                        .map(|(field, value)| (field.clone(), Scalar::from_json(Some(value))))
                        .collect(),
                );
            }
        }

        Ok(projection)
    }
}

/// Replaces every `anon-<64 hex digits>` with `anon-*`. The digest in an
/// anonymous function's name is computed over each version's own token text
/// (the Go version quotes the leaves, Rust adds statement terminators), so
/// it is not comparable; the name around it still is.
fn mask_anonymous_digests(text: &str) -> String {
    const MARKER: &str = "anon-";
    const DIGEST_LEN: usize = 64;

    let mut masked = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(at) = rest.find(MARKER) {
        let (before, after) = rest.split_at(at + MARKER.len());
        let digits = after.bytes().take_while(u8::is_ascii_hexdigit).count();

        masked.push_str(before);

        if digits >= DIGEST_LEN {
            masked.push('*');
            rest = &after[DIGEST_LEN..];
        } else {
            rest = after;
        }
    }

    masked.push_str(rest);

    masked
}

/// Picks `names` out of a JSON object; absent or null fields project as null.
fn pick(object: &Value, names: &[&str]) -> Fields {
    names
        .iter()
        .map(|name| ((*name).to_owned(), Scalar::from_json(object.get(*name))))
        .collect()
}

fn is_rust_entry(entry: &Value) -> bool {
    entry["path"]
        .as_str()
        .is_some_and(|path| path.ends_with(".rs"))
}

/// Subtracts one removed file from the coverage totals: the overall row and
/// the row of its category lose it under `discovered` and its status.
fn remove_from_coverage(coverage: &mut Value, file: &Value) {
    let status = match file["status"].as_str().unwrap_or_default() {
        "analyzed" => "analyzed",
        "excluded" => "excluded",
        "unsupported" => "unsupported",
        "read_error" => "read_errors",
        "parse_error" => "parse_errors",
        _ => "",
    };
    let category = file["category"].as_str().unwrap_or_default();

    for field in ["discovered", status] {
        if field.is_empty() {
            continue;
        }

        decrement(&mut coverage[field]);

        if let Some(by_category) = coverage["by_category"].as_array_mut() {
            for row in by_category
                .iter_mut()
                .filter(|row| row["category"] == category)
            {
                decrement(&mut row[field]);
            }
        }
    }
}

fn decrement(count: &mut Value) {
    if let Some(value) = count.as_i64() {
        *count = Value::from(value - 1);
    }
}

/// Function-derived totals of one metrics bucket.
#[derive(Default)]
struct BucketTotals {
    functions: i64,
    cyclomatic: i64,
    max_nesting: i64,
    mass: f64,
    high_complexity_functions: i64,
    high_complexity_mass: f64,
}

/// The code lines a removed analyzed file contributed to `bucket`. A
/// production file's inline-test lines were counted in the test bucket, so
/// they leave test, and only the rest leaves production.
fn bucket_lines(file: &Value, bucket: &str) -> i64 {
    let code_lines = file["code_lines"].as_i64().unwrap_or_default();
    let inline_test = file["inline_test_code_lines"].as_i64().unwrap_or_default();

    match (bucket, file["category"].as_str().unwrap_or_default()) {
        ("all", _) | ("test", "test") => code_lines,
        ("production", "production") => code_lines - inline_test,
        ("test", "production") => inline_test,
        _ => 0,
    }
}

/// Rewrites the three metrics buckets of `report` for a report whose `.rs`
/// files and functions were removed: code lines lose the removed analyzed
/// files' share of each bucket, and every function-derived total is
/// recomputed from what is left.
fn recompute_metrics(report: &mut Value, removed: &[Value]) {
    for bucket in ["all", "production", "test"] {
        let in_bucket = |category: &str| bucket == "all" || category == bucket;
        let removed_lines: i64 = removed
            .iter()
            .filter(|file| file["status"] == "analyzed")
            .map(|file| bucket_lines(file, bucket))
            .sum();
        let mut totals = BucketTotals::default();

        for function in report["functions"].as_array().into_iter().flatten() {
            if !in_bucket(function["category"].as_str().unwrap_or_default()) {
                continue;
            }

            let mass = function["mass"].as_f64().unwrap_or_default();

            totals.functions += 1;
            totals.cyclomatic += function["cyclomatic"].as_i64().unwrap_or_default();
            totals.max_nesting = totals
                .max_nesting
                .max(function["max_nesting"].as_i64().unwrap_or_default());
            totals.mass += mass;

            if function["high_complexity"].as_bool().unwrap_or_default() {
                totals.high_complexity_functions += 1;
                totals.high_complexity_mass += mass;
            }
        }

        let metrics = &mut report["metrics"][bucket];

        if let Some(lines) = metrics["code_lines"].as_i64() {
            metrics["code_lines"] = Value::from(lines - removed_lines);
        }

        metrics["functions"] = Value::from(totals.functions);
        metrics["cyclomatic"] = Value::from(totals.cyclomatic);
        metrics["max_nesting"] = Value::from(totals.max_nesting);
        metrics["mass"] = Value::from(totals.mass);
        metrics["high_complexity_functions"] = Value::from(totals.high_complexity_functions);
        metrics["high_complexity_mass"] = Value::from(totals.high_complexity_mass);
        metrics["high_complexity_mass_share"] = if totals.mass == 0.0 {
            Value::Null
        } else {
            Value::from(totals.high_complexity_mass / totals.mass)
        };
    }
}

/// The report without its `.rs` files, functions, and findings, or `None`
/// when it has none. The Go version never analyzed Rust (it reports `.rs`
/// as unsupported), so those entries can only differ; the coverage and
/// metrics totals lose their share too.
fn without_rust(report: &Value) -> Option<Value> {
    if !report["files"].as_array()?.iter().any(is_rust_entry) {
        return None;
    }

    let mut filtered = report.clone();
    let removed: Vec<Value> = report["files"]
        .as_array()?
        .iter()
        .filter(|file| is_rust_entry(file))
        .cloned()
        .collect();

    for section in ["files", "functions", "findings"] {
        if let Some(entries) = filtered[section].as_array_mut() {
            entries.retain(|entry| !is_rust_entry(entry));
        }
    }

    for file in &removed {
        remove_from_coverage(&mut filtered["coverage"], file);
    }

    recompute_metrics(&mut filtered, &removed);

    Some(filtered)
}

/// Reduces a JSON audit report (Go or Rust) to its projection.
///
/// Fields projected today: every file's path, category, status, lines, and
/// code lines; the coverage totals and per-category rows; every metric
/// bucket's code lines and function-derived totals; and each function's
/// range, SLOC, cyclomatic, nesting, mass, signals, and flags, keyed by
/// path, name, and occurrence index; and each finding's range, owner, and
/// ambiguity, keyed by path, rule, and occurrence index. `.rs` files and
/// everything measured in them are left out first (see [`without_rust`]).
pub(crate) fn project(report: &Value) -> Projection {
    let filtered = without_rust(report);
    let report = filtered.as_ref().unwrap_or(report);
    let mut projection = Projection::default();

    for file in report["files"].as_array().into_iter().flatten() {
        let path = file["path"].as_str().unwrap_or_default();
        projection.insert(
            "files",
            path,
            pick(file, &["category", "status", "lines", "code_lines"]),
        );
    }

    let coverage_counts = [
        "discovered",
        "analyzed",
        "excluded",
        "unsupported",
        "read_errors",
        "parse_errors",
    ];
    let coverage = &report["coverage"];
    let mut total = pick(coverage, &coverage_counts);
    total.extend(pick(coverage, &["symlinks", "excluded_directories"]));
    projection.insert("coverage", "total", total);

    for row in coverage["by_category"].as_array().into_iter().flatten() {
        let category = row["category"].as_str().unwrap_or_default();
        projection.insert(
            "coverage",
            &format!("category:{category}"),
            pick(row, &coverage_counts),
        );
    }

    for bucket in ["all", "production", "test"] {
        projection.insert(
            "metrics",
            bucket,
            pick(
                &report["metrics"][bucket],
                &[
                    "code_lines",
                    "functions",
                    "cyclomatic",
                    "max_nesting",
                    "mass",
                    "high_complexity_functions",
                    "high_complexity_mass",
                    "high_complexity_mass_share",
                ],
            ),
        );
    }

    for function in report["functions"].as_array().into_iter().flatten() {
        let key = format!(
            "{}::{}",
            function["path"].as_str().unwrap_or_default(),
            mask_anonymous_digests(function["name"].as_str().unwrap_or_default())
        );
        let mut fields = pick(
            function,
            &[
                "sloc",
                "cyclomatic",
                "max_nesting",
                "mass",
                "high_complexity",
            ],
        );

        // The Rust report omits a false flag; Go writes it.
        fields.insert(
            "identity_ambiguous".to_owned(),
            Scalar::Bool(function["identity_ambiguous"].as_bool().unwrap_or(false)),
        );

        for (prefix, position) in [("start", &function["start"]), ("end", &function["end"])] {
            for part in ["line", "column"] {
                fields.insert(
                    format!("{prefix}_{part}"),
                    Scalar::from_json(position.get(part)),
                );
            }
        }

        for signal in FUNCTION_SIGNALS {
            fields.insert(
                format!("signal_{signal}"),
                Scalar::from_json(function["structural_signals"].get(*signal)),
            );
        }

        projection.insert("functions", &key, fields);
    }

    for finding in report["findings"].as_array().into_iter().flatten() {
        let key = format!(
            "{}::{}",
            finding["path"].as_str().unwrap_or_default(),
            finding["rule"].as_str().unwrap_or_default()
        );
        let mut fields = Fields::new();

        for (prefix, position) in [("start", &finding["start"]), ("end", &finding["end"])] {
            for part in ["line", "column"] {
                fields.insert(
                    format!("{prefix}_{part}"),
                    Scalar::from_json(position.get(part)),
                );
            }
        }

        // The owner is the only detail both versions agree on for every rule;
        // `condition` text differs by design (go/printer layout).
        let owner = finding["details"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|detail| detail["key"] == "owner")
            .map(|detail| detail.get("value"));
        let owner = match Scalar::from_json(owner.flatten()) {
            Scalar::Text(name) => Scalar::Text(mask_anonymous_digests(&name)),
            other => other,
        };
        fields.insert("owner".to_owned(), owner);
        fields.insert(
            "identity_ambiguous".to_owned(),
            Scalar::Bool(finding["identity_ambiguous"].as_bool().unwrap_or(false)),
        );

        projection.insert("findings", &key, fields);
    }

    projection
}

/// One field-level disagreement between the Go and Rust projections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Difference {
    pub(crate) section: String,
    pub(crate) key: String,
    /// The field name, or `<entry>` when one side lacks the whole entry.
    pub(crate) field: String,
    pub(crate) go: String,
    pub(crate) rust: String,
}

impl fmt::Display for Difference {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}: {}: {} go={} rust={}",
            self.section, self.key, self.field, self.go, self.rust
        )
    }
}

/// Every disagreement between `go` and `rust`, in section and key order.
pub(crate) fn diff(go: &Projection, rust: &Projection) -> Vec<Difference> {
    let mut differences = Vec::new();
    let sections: std::collections::BTreeSet<&&str> =
        go.sections.keys().chain(rust.sections.keys()).collect();
    let empty = BTreeMap::new();

    for section in sections {
        let go_entries = go.sections.get(*section).unwrap_or(&empty);
        let rust_entries = rust.sections.get(*section).unwrap_or(&empty);
        let keys: std::collections::BTreeSet<&String> =
            go_entries.keys().chain(rust_entries.keys()).collect();

        for key in keys {
            let difference = |field: &str, go: String, rust: String| Difference {
                section: (*section).to_owned(),
                key: key.clone(),
                field: field.to_owned(),
                go,
                rust,
            };

            let (go_fields, rust_fields) = match (go_entries.get(key), rust_entries.get(key)) {
                (Some(go_fields), Some(rust_fields)) => (go_fields, rust_fields),
                (Some(_), None) => {
                    differences.push(difference("<entry>", "present".into(), "missing".into()));

                    continue;
                }
                (None, Some(_)) => {
                    differences.push(difference("<entry>", "missing".into(), "present".into()));

                    continue;
                }
                (None, None) => continue,
            };

            let names: std::collections::BTreeSet<&String> =
                go_fields.keys().chain(rust_fields.keys()).collect();

            for name in names {
                let go_value = go_fields.get(name).unwrap_or(&Scalar::Null);
                let rust_value = rust_fields.get(name).unwrap_or(&Scalar::Null);

                if !go_value.equivalent(rust_value) {
                    differences.push(difference(
                        name,
                        go_value.to_string(),
                        rust_value.to_string(),
                    ));
                }
            }
        }
    }

    differences
}

/// A divergence that is allowed to remain, with the reason it is allowed.
/// Entries must be narrow: one section, one entry key, one field.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Accepted {
    pub(crate) section: String,
    pub(crate) key: String,
    pub(crate) field: String,
    pub(crate) reason: String,
}

impl Accepted {
    pub(crate) fn covers(&self, difference: &Difference) -> bool {
        self.section == difference.section
            && self.key == difference.key
            && self.field == difference.field
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct AcceptedList {
    #[serde(default)]
    pub(crate) accepted: Vec<Accepted>,
}

#[cfg(test)]
mod tests {
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
                    "high_complexity_mass_share": 0.5},
                "production": {"code_lines": 10, "functions": 2, "cyclomatic": 13, "max_nesting": 4,
                    "mass": 13.0, "high_complexity_functions": 1, "high_complexity_mass": 11.0,
                    "high_complexity_mass_share": 0.5},
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

    #[test]
    fn rust_files_and_their_totals_are_left_out() {
        let projection = project(&mixed_report());
        let go_only = json!({
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
        });

        assert_eq!(projection.len("files"), 1);
        assert_eq!(projection.len("functions"), 1);
        assert_eq!(projection.len("findings"), 0);
        assert!(diff(&project(&go_only), &projection).is_empty());
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
            differences.iter().any(|d| d.key == "a.go#1"
                && d.field == "code_lines"
                && d.go == "5"
                && d.rust == "4")
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
}
