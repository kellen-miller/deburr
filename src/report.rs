//! The serialized schema-4 report, review ledger, and comparison result.
//!
//! Every type here is `Serialize + Deserialize`. Decoding rejects unknown
//! fields, so a report from a different schema fails loudly instead of being
//! silently truncated. Field names follow the Go report where they still
//! apply; duplication fields that described the external tool are gone.

use serde::{Deserialize, Serialize};

/// Report schema version written by this analyzer.
pub const SCHEMA_VERSION: &str = "4";

/// Review ledger schema version.
pub const LEDGER_SCHEMA_VERSION: &str = "1";

/// Analyzer identifier written to every report.
pub const ANALYZER_ID: &str = "deburr";

/// Cyclomatic complexity above which a function is "high complexity".
pub const HIGH_COMPLEXITY_THRESHOLD: u32 = 10;

/// Default `--min-clone-tokens`.
pub const DEFAULT_MIN_CLONE_TOKENS: u32 = 80;

/// Default `--min-clone-lines`.
pub const DEFAULT_MIN_CLONE_LINES: u32 = 8;

fn is_false(value: &bool) -> bool {
    !*value
}

/// Why a decoded report is unusable.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error("unsupported report schema_version {0:?}; want {SCHEMA_VERSION:?}")]
    SchemaVersion(String),
    #[error("report analyzer id and version must be non-empty")]
    Analyzer,
    #[error("report config.language must be non-empty")]
    ConfigLanguage,
    #[error("report scope.languages must be non-empty")]
    ScopeLanguages,
}

/// A complete audit result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub duplication: Duplication,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<ReviewLedger>,
    pub provenance: Provenance,
    pub analyzer: AnalyzerIdentity,
    pub schema_version: String,
    pub files: Vec<File>,
    pub functions: Vec<Function>,
    pub findings: Vec<Finding>,
    pub scope: Scope,
    pub metrics: Metrics,
    pub config: ConfigSummary,
    pub coverage: Coverage,
}

impl Report {
    /// Checks the invariants every consumer of a decoded report relies on.
    ///
    /// # Errors
    ///
    /// Returns the first violated invariant: unsupported schema version,
    /// empty analyzer identity, empty config language, or empty scope languages.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(ValidationError::SchemaVersion(self.schema_version.clone()));
        }

        if self.analyzer.id.is_empty() || self.analyzer.version.is_empty() {
            return Err(ValidationError::Analyzer);
        }

        if self.config.language.is_empty() {
            return Err(ValidationError::ConfigLanguage);
        }

        if self.scope.languages.is_empty() {
            return Err(ValidationError::ScopeLanguages);
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalyzerIdentity {
    pub id: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub source: SourceProvenance,
    pub build: BuildProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceProvenance {
    /// Git commit of the audited tree; empty when unknown.
    pub commit: String,
    /// `sha256:<hex>` over every analyzed path and its bytes.
    pub manifest_digest: String,
    pub dirty: bool,
    pub git_known: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildProvenance {
    /// The deburr build's own version.
    pub version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Production,
    Test,
    Testdata,
    Generated,
    Vendor,
    Unsupported,
}

impl Category {
    /// Fixed order used by `coverage.by_category`.
    pub const ALL: [Self; 6] = [
        Self::Production,
        Self::Test,
        Self::Testdata,
        Self::Generated,
        Self::Vendor,
        Self::Unsupported,
    ];

    /// The serialized name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Production => "production",
            Self::Test => "test",
            Self::Testdata => "testdata",
            Self::Generated => "generated",
            Self::Vendor => "vendor",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    Analyzed,
    Excluded,
    Unsupported,
    ReadError,
    ParseError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    pub path: String,
    pub category: Category,
    pub status: FileStatus,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub error: String,
    pub bytes: u64,
    pub lines: u32,
    pub code_lines: u32,
    /// Code lines inside the file's inline test regions (Rust `#[cfg(test)]`
    /// items and `#[test]` functions); always zero for other languages. They
    /// are part of `code_lines`.
    pub inline_test_code_lines: u32,
}

/// 1-based line and 1-based byte column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Position {
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StructuralSignals {
    pub flat_guards: u32,
    pub field_mappings: u32,
    pub nested_branches: u32,
    pub assertion_like_calls: u32,
    /// Rust only: `.clone()`, `.to_owned()`, `.cloned()` with no arguments.
    pub clone_like_calls: u32,
    /// Rust only: `.unwrap()`-style calls and `.expect(..)`-style calls.
    pub unwrap_like_calls: u32,
    /// Rust only: `panic!`, `todo!`, `unimplemented!`, `unreachable!`.
    pub panic_macros: u32,
    /// Rust only: `unsafe` blocks.
    pub unsafe_blocks: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Function {
    /// `<package>/<path>::<identity>`; unique within a report.
    pub id: String,
    /// `sha256:<hex>` over the function's token text.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub fingerprint: String,
    pub path: String,
    pub category: Category,
    pub name: String,
    pub structural_signals: StructuralSignals,
    pub start: Position,
    pub end: Position,
    pub sloc: u32,
    pub cyclomatic: u32,
    pub max_nesting: u32,
    pub mass: f64,
    pub high_complexity: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub identity_ambiguous: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Detail {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub fingerprint: String,
    pub rule: String,
    pub path: String,
    pub severity: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<Detail>,
    pub start: Position,
    pub end: Position,
    #[serde(default, skip_serializing_if = "is_false")]
    pub identity_ambiguous: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub path_mode: String,
    pub languages: Vec<String>,
    pub include_tests: bool,
    pub include_testdata: bool,
    pub include_vendor: bool,
    pub include_generated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleConfig {
    pub id: String,
    pub enabled: bool,
}

/// Duplication settings recorded in the report config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DuplicationConfig {
    pub requested: bool,
    pub min_tokens: u32,
    pub min_lines: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigSummary {
    pub rules: Vec<RuleConfig>,
    pub language: String,
    pub duplication: DuplicationConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_file_bytes: Option<u64>,
    pub high_complexity_threshold: u32,
    pub include_tests: bool,
    pub include_testdata: bool,
    pub include_vendor: bool,
    pub include_generated: bool,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricBucket {
    /// `null` when the bucket has no mass.
    pub high_complexity_mass_share: Option<f64>,
    pub code_lines: u32,
    pub functions: u32,
    pub cyclomatic: u32,
    pub max_nesting: u32,
    pub mass: f64,
    pub high_complexity_functions: u32,
    pub high_complexity_mass: f64,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metrics {
    pub all: MetricBucket,
    pub production: MetricBucket,
    pub test: MetricBucket,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CategoryCoverage {
    pub category: Category,
    pub discovered: u32,
    pub analyzed: u32,
    pub excluded: u32,
    pub unsupported: u32,
    pub read_errors: u32,
    pub parse_errors: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Coverage {
    pub by_category: Vec<CategoryCoverage>,
    pub discovered: u32,
    pub analyzed: u32,
    pub excluded: u32,
    pub unsupported: u32,
    pub read_errors: u32,
    pub parse_errors: u32,
    pub symlinks: u32,
    pub excluded_directories: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DuplicationStatus {
    NotRequested,
    Measured,
    Error,
}

/// Per-category duplication measurement. Percentage is distinct duplicated
/// code lines over the category's code lines, times 100 (never above 100).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DuplicationCategory {
    pub category: Category,
    pub files: u32,
    pub code_lines: u32,
    pub duplicated_code_lines: u32,
    pub percentage: f64,
}

/// One side of a clone pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloneEndpoint {
    pub path: String,
    pub start: Position,
    pub end: Position,
    pub code_lines: u32,
    /// IDs of the report functions this region overlaps (innermost per
    /// overlapped region); empty for file-level code. Every ID resolves to
    /// an entry in the same report's `functions`.
    pub functions: Vec<String>,
}

/// A pair of code regions with the same token sequence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodeClone {
    pub id: String,
    pub family_id: String,
    pub fingerprint: String,
    pub category: Category,
    pub tokens: u32,
    #[serde(default, skip_serializing_if = "is_false")]
    pub identity_ambiguous: bool,
    pub endpoints: [CloneEndpoint; 2],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Duplication {
    pub status: DuplicationStatus,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub error: String,
    pub min_tokens: u32,
    pub min_lines: u32,
    pub categories: Vec<DuplicationCategory>,
    pub clones: Vec<CodeClone>,
}

/// A source location used by review items.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Location {
    pub path: String,
    pub start: Position,
    pub end: Position,
}

/// Inventory kinds in ledger sort order: clone, finding, function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewKind {
    Clone,
    Finding,
    Function,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewStatus {
    Unreviewed,
    Refactored,
    Retained,
    Deferred,
}

/// A recorded decision, kept as `previous` when a stale item is reset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewDecision {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub fingerprint: String,
    pub status: ReviewStatus,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason_category: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub evidence: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub upstream: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous: Option<ReviewDecision>,
    pub id: String,
    pub kind: ReviewKind,
    pub fingerprint: String,
    pub status: ReviewStatus,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason_category: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub evidence: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub upstream: String,
    pub locations: Vec<Location>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub stale: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub identity_ambiguous: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewCounts {
    pub total: u32,
    pub unreviewed: u32,
    pub refactored: u32,
    pub retained: u32,
    pub deferred: u32,
    pub stale: u32,
}

/// The review ledger: one item per finding, high-complexity function, and clone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewLedger {
    pub schema_version: String,
    /// `sha256:<hex>` over analyzer, schema version, scope, and config.
    pub report_identity: String,
    pub source_manifest_digest: String,
    pub items: Vec<ReviewItem>,
    pub counts: ReviewCounts,
}

/// A report with a validated ledger attached, as produced by `review apply`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ReviewedReport(pub Report);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeState {
    Matched,
    New,
    Removed,
    Ambiguous,
    NoLongerReported,
    SourceRemoved,
    DeletedFunction,
    ResolvedThreshold,
    CrossedThreshold,
    Incompatible,
    Incomplete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchBasis {
    Id,
    Fingerprint,
}

/// How one function, finding, or clone moved between two reports.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, bound(deserialize = "T: Deserialize<'de>"))]
pub struct Change<T> {
    pub state: ChangeState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub match_basis: Option<MatchBasis>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<T>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<T>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_review: Option<ReviewItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_review: Option<ReviewItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub before_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub after_ids: Vec<String>,
}

pub type FunctionChange = Change<Function>;
pub type FindingChange = Change<Finding>;
pub type CloneChange = Change<CodeClone>;

/// Metric differences (`after - before`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricDelta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub high_complexity_mass_share: Option<f64>,
    pub code_lines: i64,
    pub functions: i64,
    pub cyclomatic: i64,
    pub max_nesting: i64,
    pub mass: f64,
    pub high_complexity_functions: i64,
    pub high_complexity_mass: f64,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricsDelta {
    pub all: MetricDelta,
    pub production: MetricDelta,
    pub test: MetricDelta,
}

/// Several before or after entries that could not be paired one-to-one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AmbiguousMatch {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<ChangeState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub match_basis: Option<MatchBasis>,
    pub rule: String,
    pub path: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub before_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub after_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DuplicationComparison {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
    pub before_status: DuplicationStatus,
    pub after_status: DuplicationStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub before_clones: Vec<CodeClone>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub after_clones: Vec<CodeClone>,
    pub before_config: DuplicationConfig,
    pub after_config: DuplicationConfig,
    pub comparable: bool,
}

/// The result of `deburr compare`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Comparison {
    /// `null` when the reports are incompatible.
    pub metrics_delta: Option<MetricsDelta>,
    pub after_provenance: Provenance,
    pub before_provenance: Provenance,
    pub before_analyzer: AnalyzerIdentity,
    pub after_analyzer: AnalyzerIdentity,
    pub schema_version: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ambiguous_clones: Vec<AmbiguousMatch>,
    pub function_changes: Vec<FunctionChange>,
    pub resolved_findings: Vec<Finding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ambiguous_functions: Vec<AmbiguousMatch>,
    pub clone_changes: Vec<CloneChange>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ambiguous_findings: Vec<AmbiguousMatch>,
    pub new_findings: Vec<Finding>,
    pub finding_changes: Vec<FindingChange>,
    pub before_scope: Scope,
    pub after_scope: Scope,
    pub duplication: DuplicationComparison,
    pub before_metrics: Metrics,
    pub after_metrics: Metrics,
    pub before_config: ConfigSummary,
    pub after_config: ConfigSummary,
    pub before_review: ReviewLedger,
    pub after_review: ReviewLedger,
    pub before_coverage: Coverage,
    pub after_coverage: Coverage,
    pub complete: bool,
    pub compatible: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::AuditOptions;

    fn empty_report() -> Report {
        let options = AuditOptions {
            include_tests: true,
            include_testdata: false,
            include_vendor: false,
            include_generated: false,
            max_file_bytes: None,
            duplication: None,
        };

        crate::audit::empty_report(&options)
    }

    #[test]
    fn report_round_trips_through_json() {
        let report = empty_report();
        let text = serde_json::to_string(&report).expect("encode");
        let decoded: Report = serde_json::from_str(&text).expect("decode");

        assert_eq!(decoded, report);
        assert_eq!(decoded.validate(), Ok(()));
    }

    #[test]
    fn decode_rejects_unknown_fields() {
        let mut value = serde_json::to_value(empty_report()).expect("encode");
        value["surprise"] = serde_json::Value::Bool(true);

        assert!(serde_json::from_value::<Report>(value).is_err());
    }

    #[test]
    fn validate_rejects_bad_reports() {
        let mut report = empty_report();
        report.schema_version = "2".to_owned();
        assert_eq!(
            report.validate(),
            Err(ValidationError::SchemaVersion("2".to_owned()))
        );

        let mut report = empty_report();
        report.analyzer.version.clear();
        assert_eq!(report.validate(), Err(ValidationError::Analyzer));

        let mut report = empty_report();
        report.config.language.clear();
        assert_eq!(report.validate(), Err(ValidationError::ConfigLanguage));

        let mut report = empty_report();
        report.scope.languages.clear();
        assert_eq!(report.validate(), Err(ValidationError::ScopeLanguages));
    }

    #[test]
    fn clone_endpoints_must_be_a_pair() {
        let endpoint = r#"{"path":"a.go","start":{"line":1,"column":1},"end":{"line":9,"column":2},"code_lines":9,"functions":[]}"#;
        let one = format!(
            r#"{{"id":"c","family_id":"f","fingerprint":"x","category":"production","tokens":80,"endpoints":[{endpoint}]}}"#
        );
        let two = format!(
            r#"{{"id":"c","family_id":"f","fingerprint":"x","category":"production","tokens":80,"endpoints":[{endpoint},{endpoint}]}}"#
        );

        assert!(serde_json::from_str::<CodeClone>(&one).is_err());
        assert!(serde_json::from_str::<CodeClone>(&two).is_ok());
    }

    #[test]
    fn review_kinds_sort_clone_finding_function() {
        let mut kinds = [ReviewKind::Function, ReviewKind::Clone, ReviewKind::Finding];
        kinds.sort();

        assert_eq!(
            kinds,
            [ReviewKind::Clone, ReviewKind::Finding, ReviewKind::Function]
        );
    }
}
