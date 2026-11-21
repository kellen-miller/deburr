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

/// The structural signals compared per function. The remaining signals
/// (`flat_guards`, `field_mappings`, `assertion_like_calls`) join this list
/// with the findings milestone, which implements them.
const FUNCTION_SIGNALS: &[&str] = &["nested_branches"];

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
}

/// Picks `names` out of a JSON object; absent or null fields project as null.
fn pick(object: &Value, names: &[&str]) -> Fields {
    names
        .iter()
        .map(|name| ((*name).to_owned(), Scalar::from_json(object.get(*name))))
        .collect()
}

/// Reduces a JSON audit report (Go or Rust) to its projection.
///
/// Fields projected today: every file's path, category, status, lines, and
/// code lines; the coverage totals and per-category rows; every metric
/// bucket's code lines and function-derived totals; and each function's
/// range, SLOC, cyclomatic, nesting, mass, signals, and flags, keyed by
/// path, name, and occurrence index. Findings join with their milestone.
pub(crate) fn project(report: &Value) -> Projection {
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
            function["name"].as_str().unwrap_or_default()
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
