//! Report and comparison output in text, JSON, HTML, and GitHub formats.
//!
//! The renderers show facts, not conclusions: every function, finding, clone,
//! review decision, and coverage count in a report appears in each format.
//! The small formatting functions here are the one place that decides how a
//! location, a review decision, or a measurement reads, so all formats agree.

use std::borrow::Cow;
use std::io::{self, Write};

use clap::ValueEnum;
use serde::Serialize;

use crate::report::{
    CloneEndpoint, CodeClone, Comparison, Finding, Function, Position, Provenance, Report,
    ReviewDecision, ReviewItem, ReviewLedger,
};
use crate::review;

pub mod github;
pub mod html;
pub mod text;

/// Output formats accepted by `--format`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Format {
    Text,
    Json,
    Html,
    Github,
}

impl Format {
    /// The name accepted by `--format`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Json => "json",
            Self::Html => "html",
            Self::Github => "github",
        }
    }
}

/// Writes `value` as pretty JSON followed by a newline.
///
/// # Errors
///
/// Returns an I/O error when encoding or writing fails.
pub fn write_json<T: Serialize>(value: &T, out: &mut dyn Write) -> io::Result<()> {
    serde_json::to_writer_pretty(&mut *out, value)?;
    out.write_all(b"\n")
}

/// Writes `report` in `format`.
///
/// # Errors
///
/// Returns an I/O error when writing fails.
pub fn render_report(report: &Report, format: Format, out: &mut dyn Write) -> io::Result<()> {
    match format {
        Format::Text => text::report(report, out),
        Format::Json => write_json(report, out),
        Format::Html => html::report(report, out),
        Format::Github => github::report(report, out),
    }
}

/// Writes `comparison` in `format`.
///
/// # Errors
///
/// Returns an I/O error when writing fails.
pub fn render_comparison(
    comparison: &Comparison,
    format: Format,
    out: &mut dyn Write,
) -> io::Result<()> {
    match format {
        Format::Text => text::comparison(comparison, out),
        Format::Json => write_json(comparison, out),
        Format::Html => html::comparison(comparison, out),
        Format::Github => github::comparison(comparison, out),
    }
}

/// The serialized (snake_case) name of a plain enum such as a file status,
/// so renderers print exactly what the JSON report contains.
pub(crate) fn enum_name<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(name)) => name,
        _ => String::new(),
    }
}

/// `path:line:column-line:column`, the location form used by every format.
pub(crate) fn location(path: &str, start: Position, end: Position) -> String {
    format!(
        "{path}:{}:{}-{}:{}",
        start.line, start.column, end.line, end.column
    )
}

/// The provenance record on one line; empty values read `unknown`.
pub(crate) fn provenance_text(provenance: &Provenance) -> String {
    let known = |value: &str| {
        if value.is_empty() {
            "unknown".to_owned()
        } else {
            value.to_owned()
        }
    };

    format!(
        "build_version={} source_commit={} source_git_known={} source_dirty={} source_manifest_digest={}",
        known(&provenance.build.version),
        known(&provenance.source.commit),
        provenance.source.git_known,
        provenance.source.dirty,
        known(&provenance.source.manifest_digest),
    )
}

/// The ledger attached to `report`, or a fresh all-unreviewed one.
///
/// An attached ledger is trusted: `review apply` validated it already.
pub(crate) fn ledger_of(report: &Report) -> Cow<'_, ReviewLedger> {
    report
        .review
        .as_ref()
        .map_or_else(|| Cow::Owned(review::init_ledger(report)), Cow::Borrowed)
}

/// Functions ranked by raw mass, then nesting, cyclomatic complexity, and
/// source position, so ties never depend on input order.
pub(crate) fn ranked_functions<'a>(
    functions: impl IntoIterator<Item = &'a Function>,
) -> Vec<&'a Function> {
    let mut ranked: Vec<&Function> = functions.into_iter().collect();
    ranked.sort_by(|left, right| {
        right
            .mass
            .total_cmp(&left.mass)
            .then_with(|| right.max_nesting.cmp(&left.max_nesting))
            .then_with(|| right.cyclomatic.cmp(&left.cyclomatic))
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.start.cmp(&right.start))
            .then_with(|| left.id.cmp(&right.id))
    });

    ranked
}

/// A recorded decision, kept as `previous` on stale review items.
pub(crate) fn previous_text(previous: &ReviewDecision) -> String {
    let mut values = vec![enum_name(&previous.status)];
    for (name, value) in [
        ("reason_category", &previous.reason_category),
        ("reason", &previous.reason),
        ("evidence", &previous.evidence),
        ("upstream", &previous.upstream),
        ("fingerprint", &previous.fingerprint),
    ] {
        if !value.is_empty() {
            values.push(format!("{name}={value}"));
        }
    }

    values.join("; ")
}

/// The decision fields of a review item as ` name=value` pairs, each with a
/// leading space, in a fixed order; empty fields are omitted.
pub(crate) fn review_fields(item: &ReviewItem) -> String {
    let mut fields = String::new();
    if item.stale {
        fields.push_str(" stale=true");
    }

    for (name, value) in [
        ("reason_category", &item.reason_category),
        ("reason", &item.reason),
        ("evidence", &item.evidence),
        ("upstream", &item.upstream),
    ] {
        if !value.is_empty() {
            fields.push_str(&format!(" {name}={value}"));
        }
    }

    if let Some(previous) = &item.previous {
        fields.push_str(&format!(" previous={}", previous_text(previous)));
    }

    fields
}

pub(crate) fn function_text(function: &Function) -> String {
    let signals = &function.structural_signals;

    format!(
        "{} name={} sloc={} cyclomatic={} nesting={} mass={:.6} high_complexity={} flat_guards={} field_mappings={} nested_branches={} assertion_like_calls={} ambiguous={}",
        location(&function.path, function.start, function.end),
        function.name,
        function.sloc,
        function.cyclomatic,
        function.max_nesting,
        function.mass,
        function.high_complexity,
        signals.flat_guards,
        signals.field_mappings,
        signals.nested_branches,
        signals.assertion_like_calls,
        function.identity_ambiguous,
    )
}

pub(crate) fn finding_text(finding: &Finding) -> String {
    format!(
        "{} [{}] {}: {} (id={} ambiguous={})",
        location(&finding.path, finding.start, finding.end),
        finding.severity,
        finding.rule,
        finding.message,
        finding.id,
        finding.identity_ambiguous,
    )
}

/// One clone endpoint: location, code lines, and the functions it overlaps.
pub(crate) fn endpoint_text(endpoint: &CloneEndpoint) -> String {
    let functions = if endpoint.functions.is_empty() {
        "-".to_owned()
    } else {
        endpoint.functions.join(",")
    };

    format!(
        "{} code_lines={} functions={functions}",
        location(&endpoint.path, endpoint.start, endpoint.end),
        endpoint.code_lines,
    )
}

pub(crate) fn clone_text(clone: &CodeClone) -> String {
    let endpoints: Vec<String> = clone.endpoints.iter().map(endpoint_text).collect();

    format!(
        "id={} family={} tokens={} ambiguous={} endpoints={}",
        clone.id,
        clone.family_id,
        clone.tokens,
        clone.identity_ambiguous,
        endpoints.join(" | "),
    )
}
