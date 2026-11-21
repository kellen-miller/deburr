//! GitHub workflow-command rendering.

use std::io::{self, Write};

use super::{enum_name, ledger_of, provenance_text, review_fields};
use crate::report::{
    ChangeState, Comparison, DuplicationStatus, FileStatus, Finding, Position, Report, ReviewItem,
    ReviewStatus,
};

/// How a workflow-command string is escaped. Property values also escape the
/// two characters that delimit properties.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Escape {
    Message,
    Property,
}

/// The source range an annotation points at.
#[derive(Clone, Copy)]
struct Span<'a> {
    path: &'a str,
    start: Position,
    end: Position,
}

fn escape(value: &str, kind: Escape) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '%' => escaped.push_str("%25"),
            '\r' => escaped.push_str("%0D"),
            '\n' => escaped.push_str("%0A"),
            ':' if kind == Escape::Property => escaped.push_str("%3A"),
            ',' if kind == Escape::Property => escaped.push_str("%2C"),
            other => escaped.push(other),
        }
    }

    escaped
}

/// Writes one `::level file=..,line=..,endLine=..,title=..::message` command.
/// Properties are omitted when absent (no span, no title, a zero line).
fn command(
    out: &mut dyn Write,
    level: &str,
    at: Option<Span<'_>>,
    title: &str,
    message: &str,
) -> io::Result<()> {
    let mut properties = Vec::new();
    if let Some(span) = at {
        properties.push(format!("file={}", escape(span.path, Escape::Property)));
        if span.start.line > 0 {
            properties.push(format!("line={}", span.start.line));
        }

        if span.end.line > 0 {
            properties.push(format!("endLine={}", span.end.line));
        }
    }

    if !title.is_empty() {
        properties.push(format!("title={}", escape(title, Escape::Property)));
    }

    let message = escape(message, Escape::Message);
    if properties.is_empty() {
        writeln!(out, "::{level}::{message}")
    } else {
        writeln!(out, "::{level} {}::{message}", properties.join(","))
    }
}

/// Writes the report as `::notice`/`::warning`/`::error` commands.
///
/// # Errors
///
/// Returns an I/O error when writing fails.
pub fn report(report: &Report, out: &mut dyn Write) -> io::Result<()> {
    command(
        out,
        "notice",
        None,
        "deburr provenance",
        &provenance_text(&report.provenance),
    )?;

    let coverage = &report.coverage;
    command(
        out,
        "notice",
        None,
        "deburr coverage",
        &format!(
            "discovered={},analyzed={},excluded={},unsupported={},read_errors={},parse_errors={},symlinks={},excluded_directories={}",
            coverage.discovered,
            coverage.analyzed,
            coverage.excluded,
            coverage.unsupported,
            coverage.read_errors,
            coverage.parse_errors,
            coverage.symlinks,
            coverage.excluded_directories,
        ),
    )?;
    if coverage.read_errors > 0 || coverage.parse_errors > 0 {
        command(
            out,
            "error",
            None,
            "deburr analysis",
            &format!(
                "analysis incomplete: read_errors={},parse_errors={}",
                coverage.read_errors, coverage.parse_errors
            ),
        )?;
    }

    if coverage.analyzed == 0 {
        command(
            out,
            "notice",
            None,
            "deburr analysis",
            "no analyzable Go or TypeScript files",
        )?;
    }

    let duplication = &report.duplication;
    command(
        out,
        "notice",
        None,
        "deburr duplication",
        &format!(
            "status={},min_tokens={},min_lines={}",
            enum_name(&duplication.status),
            duplication.min_tokens,
            duplication.min_lines
        ),
    )?;
    if duplication.status == DuplicationStatus::Error {
        command(out, "error", None, "deburr duplication", &duplication.error)?;
    }

    if duplication.status == DuplicationStatus::Measured {
        for measured in &duplication.categories {
            command(
                out,
                "notice",
                None,
                "deburr duplication",
                &format!(
                    "{}: {:.2}% duplication ({} of {} code lines across {} files)",
                    measured.category.as_str(),
                    measured.percentage,
                    measured.duplicated_code_lines,
                    measured.code_lines,
                    measured.files,
                ),
            )?;
        }
    }

    for file in &report.files {
        if !matches!(file.status, FileStatus::ReadError | FileStatus::ParseError) {
            continue;
        }

        let message = if file.error.is_empty() {
            &file.reason
        } else {
            &file.error
        };
        let span = Span {
            path: &file.path,
            start: Position::default(),
            end: Position::default(),
        };
        command(out, "error", Some(span), "deburr analysis", message)?;
    }

    for function in &report.functions {
        let signals = &function.structural_signals;
        let span = Span {
            path: &function.path,
            start: function.start,
            end: function.end,
        };
        command(
            out,
            "notice",
            Some(span),
            "deburr function",
            &format!(
                "{} sloc={} cyclomatic={} nesting={} mass={:.6} flat_guards={} field_mappings={} nested_branches={} assertion_like_calls={} identity_ambiguous={}",
                function.name,
                function.sloc,
                function.cyclomatic,
                function.max_nesting,
                function.mass,
                signals.flat_guards,
                signals.field_mappings,
                signals.nested_branches,
                signals.assertion_like_calls,
                function.identity_ambiguous,
            ),
        )?;
    }

    for finding in &report.findings {
        let level = match finding.severity.to_lowercase().as_str() {
            "error" => "error",
            "notice" => "notice",
            _ => "warning",
        };
        finding_command(out, level, finding, "")?;
    }

    if duplication.status == DuplicationStatus::Measured {
        for clone in &duplication.clones {
            for endpoint in &clone.endpoints {
                let span = Span {
                    path: &endpoint.path,
                    start: endpoint.start,
                    end: endpoint.end,
                };
                command(
                    out,
                    "warning",
                    Some(span),
                    "deburr duplication",
                    &format!(
                        "clone {} family={} tokens={} identity_ambiguous={} code_lines={} functions={}",
                        clone.id,
                        clone.family_id,
                        clone.tokens,
                        clone.identity_ambiguous,
                        endpoint.code_lines,
                        endpoint.functions.join(","),
                    ),
                )?;
            }
        }
    }

    let ledger = ledger_of(report);
    let counts = &ledger.counts;
    command(
        out,
        "notice",
        None,
        "deburr review",
        &format!(
            "total={},unreviewed={},refactored={},retained={},deferred={},stale={}",
            counts.total,
            counts.unreviewed,
            counts.refactored,
            counts.retained,
            counts.deferred,
            counts.stale,
        ),
    )?;
    for item in &ledger.items {
        review_command(out, item)?;
    }

    Ok(())
}

/// Writes the comparison as workflow commands.
///
/// # Errors
///
/// Returns an I/O error when writing fails.
pub fn comparison(comparison: &Comparison, out: &mut dyn Write) -> io::Result<()> {
    command(
        out,
        "notice",
        None,
        "deburr comparison before provenance",
        &provenance_text(&comparison.before_provenance),
    )?;
    command(
        out,
        "notice",
        None,
        "deburr comparison after provenance",
        &provenance_text(&comparison.after_provenance),
    )?;
    command(
        out,
        "notice",
        None,
        "deburr comparison",
        &format!(
            "compatible={},complete={},before_analyzed={},after_analyzed={}",
            comparison.compatible,
            comparison.complete,
            comparison.before_coverage.analyzed,
            comparison.after_coverage.analyzed,
        ),
    )?;
    if !comparison.compatible || !comparison.complete {
        let message = if comparison.reason.is_empty() {
            "comparison unavailable"
        } else {
            &comparison.reason
        };
        command(out, "error", None, "deburr comparison", message)?;
    }

    let (before, after) = (
        &comparison.before_review.counts,
        &comparison.after_review.counts,
    );
    command(
        out,
        "notice",
        None,
        "deburr comparison review",
        &format!(
            "before_total={},before_unreviewed={},before_refactored={},before_retained={},before_deferred={},before_stale={},after_total={},after_unreviewed={},after_refactored={},after_retained={},after_deferred={},after_stale={}",
            before.total,
            before.unreviewed,
            before.refactored,
            before.retained,
            before.deferred,
            before.stale,
            after.total,
            after.unreviewed,
            after.refactored,
            after.retained,
            after.deferred,
            after.stale,
        ),
    )?;

    for change in &comparison.finding_changes {
        match change.state {
            ChangeState::New => {
                if let Some(finding) = &change.after {
                    finding_command(out, "warning", finding, "new: ")?;
                }
            }
            ChangeState::NoLongerReported | ChangeState::SourceRemoved => {
                if let Some(finding) = &change.before {
                    let prefix = format!("{}: ", enum_name(&change.state));
                    finding_command(out, "notice", finding, &prefix)?;
                }
            }
            ChangeState::Ambiguous => ambiguous_command(
                out,
                "finding",
                change.state,
                &change.before_ids,
                &change.after_ids,
            )?,
            _ => {}
        }
    }

    for change in &comparison.function_changes {
        let Some(function) = change.after.as_ref().or(change.before.as_ref()) else {
            continue;
        };

        let level = if matches!(
            change.state,
            ChangeState::New | ChangeState::CrossedThreshold
        ) {
            "warning"
        } else {
            "notice"
        };
        let span = Span {
            path: &function.path,
            start: function.start,
            end: function.end,
        };
        command(
            out,
            level,
            Some(span),
            "deburr function comparison",
            &format!(
                "{} state={}; no behavior conclusion",
                function.name,
                enum_name(&change.state)
            ),
        )?;
        if change.state == ChangeState::Ambiguous {
            ambiguous_command(
                out,
                "function",
                change.state,
                &change.before_ids,
                &change.after_ids,
            )?;
        }
    }

    for change in &comparison.clone_changes {
        let Some(clone) = change.after.as_ref().or(change.before.as_ref()) else {
            continue;
        };

        for endpoint in &clone.endpoints {
            let span = Span {
                path: &endpoint.path,
                start: endpoint.start,
                end: endpoint.end,
            };
            command(
                out,
                "notice",
                Some(span),
                "deburr clone comparison",
                &format!(
                    "clone {} family={} state={} identity_ambiguous={} functions={}",
                    clone.id,
                    clone.family_id,
                    enum_name(&change.state),
                    clone.identity_ambiguous,
                    endpoint.functions.join(","),
                ),
            )?;
        }

        if change.state == ChangeState::Ambiguous {
            ambiguous_command(
                out,
                "clone",
                change.state,
                &change.before_ids,
                &change.after_ids,
            )?;
        }
    }

    for (kind, matches) in [
        ("finding", &comparison.ambiguous_findings),
        ("function", &comparison.ambiguous_functions),
        ("clone", &comparison.ambiguous_clones),
    ] {
        if !matches.is_empty() {
            command(
                out,
                "notice",
                None,
                "deburr comparison",
                &format!(
                    "{} ambiguous {kind} matches; no new or resolved claim made",
                    matches.len()
                ),
            )?;
        }
    }

    let duplication = &comparison.duplication;
    if !duplication.comparable {
        command(
            out,
            "notice",
            None,
            "deburr duplication",
            &format!(
                "before={},after={},{}",
                enum_name(&duplication.before_status),
                enum_name(&duplication.after_status),
                duplication.reason
            ),
        )?;
    }

    Ok(())
}

/// Rewrites every path in `report` to sit under `prefix`, the audited
/// directory relative to the repository root, so annotations land on the
/// right files when the audit ran in a subdirectory. An empty prefix leaves
/// the report unchanged.
#[must_use]
pub fn with_path_prefix(report: &Report, prefix: &str) -> Report {
    let mut prefixed = report.clone();
    let prefix = prefix.trim_matches('/');
    if prefix.is_empty() {
        return prefixed;
    }

    let join = |path: &mut String| *path = format!("{prefix}/{path}");
    for file in &mut prefixed.files {
        join(&mut file.path);
    }

    for function in &mut prefixed.functions {
        join(&mut function.path);
    }

    for finding in &mut prefixed.findings {
        join(&mut finding.path);
    }

    for clone in &mut prefixed.duplication.clones {
        for endpoint in &mut clone.endpoints {
            join(&mut endpoint.path);
        }
    }

    if let Some(ledger) = &mut prefixed.review {
        for item in &mut ledger.items {
            for at in &mut item.locations {
                join(&mut at.path);
            }
        }
    }

    prefixed
}

fn finding_command(
    out: &mut dyn Write,
    level: &str,
    finding: &Finding,
    prefix: &str,
) -> io::Result<()> {
    let span = Span {
        path: &finding.path,
        start: finding.start,
        end: finding.end,
    };

    command(
        out,
        level,
        Some(span),
        &finding.rule,
        &format!(
            "{prefix}{} identity_ambiguous={}",
            finding.message, finding.identity_ambiguous
        ),
    )
}

fn ambiguous_command(
    out: &mut dyn Write,
    kind: &str,
    state: ChangeState,
    before: &[String],
    after: &[String],
) -> io::Result<()> {
    command(
        out,
        "notice",
        None,
        "deburr comparison",
        &format!(
            "{kind} {} before={} after={}; no change claim made",
            enum_name(&state),
            before.join(","),
            after.join(",")
        ),
    )
}

/// Decided or stale review items become annotations; untouched ones are
/// covered by the summary line.
fn review_command(out: &mut dyn Write, item: &ReviewItem) -> io::Result<()> {
    if item.status == ReviewStatus::Unreviewed && !item.stale {
        return Ok(());
    }

    let at = item.locations.first().map(|first| Span {
        path: &first.path,
        start: first.start,
        end: first.end,
    });
    let mut message = format!(
        "{} {} status={}",
        enum_name(&item.kind),
        item.id,
        enum_name(&item.status)
    );
    message.push_str(&review_fields(item));

    command(out, "notice", at, "deburr review", &message)
}
