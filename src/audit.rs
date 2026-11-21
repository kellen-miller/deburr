//! Runs one audit from start to finish and returns its report.

use std::io::Write;
use std::path::Path;

use crate::duplication::DuplicationSettings;
use crate::report::{
    ANALYZER_ID, AnalyzerIdentity, ConfigSummary, Coverage, DEFAULT_MIN_CLONE_LINES,
    DEFAULT_MIN_CLONE_TOKENS, Duplication, DuplicationConfig, DuplicationStatus,
    HIGH_COMPLEXITY_THRESHOLD, Metrics, Provenance, Report, RuleConfig, SCHEMA_VERSION, Scope,
};

/// What an audit includes.
#[derive(Debug, Clone)]
pub struct AuditOptions {
    pub include_tests: bool,
    pub include_testdata: bool,
    pub include_vendor: bool,
    pub include_generated: bool,
    pub max_file_bytes: Option<u64>,
    /// `None` means `--no-duplicates`.
    pub duplication: Option<DuplicationSettings>,
}

/// A problem that makes the audit exit 1 after its report is written.
#[derive(Debug, thiserror::Error)]
pub enum AuditError {
    #[error("audit is not implemented yet")]
    NotImplemented,
    #[error("{path}: {message}")]
    File { path: String, message: String },
    #[error("{0}")]
    Input(String),
}

/// An audit always yields a report; non-empty `errors` means exit 1.
#[derive(Debug)]
pub struct AuditOutcome {
    pub report: Report,
    pub errors: Vec<AuditError>,
}

/// Audits the tree at `root`.
///
/// Not implemented yet: returns an empty report carrying the requested
/// scope and config, plus a "not implemented yet" error.
pub fn audit(_root: &Path, options: &AuditOptions, _progress: &mut dyn Write) -> AuditOutcome {
    AuditOutcome {
        report: empty_report(options),
        errors: vec![AuditError::NotImplemented],
    }
}

pub(crate) fn empty_report(options: &AuditOptions) -> Report {
    let duplication_config = options.duplication.map_or(
        DuplicationConfig {
            requested: false,
            min_tokens: DEFAULT_MIN_CLONE_TOKENS,
            min_lines: DEFAULT_MIN_CLONE_LINES,
        },
        |settings| DuplicationConfig {
            requested: true,
            min_tokens: u32::try_from(settings.min_tokens).unwrap_or(u32::MAX),
            min_lines: u32::try_from(settings.min_lines).unwrap_or(u32::MAX),
        },
    );

    Report {
        duplication: Duplication {
            status: DuplicationStatus::NotRequested,
            error: String::new(),
            min_tokens: duplication_config.min_tokens,
            min_lines: duplication_config.min_lines,
            categories: Vec::new(),
            clones: Vec::new(),
        },
        review: None,
        provenance: Provenance::default(),
        analyzer: AnalyzerIdentity {
            id: ANALYZER_ID.to_owned(),
            version: crate::VERSION.to_owned(),
        },
        schema_version: SCHEMA_VERSION.to_owned(),
        files: Vec::new(),
        functions: Vec::new(),
        findings: Vec::new(),
        scope: Scope {
            path_mode: "source-relative".to_owned(),
            languages: vec!["go".to_owned(), "typescript".to_owned()],
            include_tests: options.include_tests,
            include_testdata: options.include_testdata,
            include_vendor: options.include_vendor,
            include_generated: options.include_generated,
        },
        metrics: Metrics::default(),
        config: ConfigSummary {
            rules: ["redundant-boolean-return", "duplicate-branches"]
                .into_iter()
                .map(|id| RuleConfig {
                    id: id.to_owned(),
                    enabled: true,
                })
                .collect(),
            language: "go,typescript".to_owned(),
            duplication: duplication_config,
            max_file_bytes: options.max_file_bytes,
            high_complexity_threshold: HIGH_COMPLEXITY_THRESHOLD,
            include_tests: options.include_tests,
            include_testdata: options.include_testdata,
            include_vendor: options.include_vendor,
            include_generated: options.include_generated,
        },
        coverage: Coverage::default(),
    }
}
