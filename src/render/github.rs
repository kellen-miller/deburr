//! GitHub workflow-command rendering.
//!
//! The report is one verdict `::notice`, annotations for the first
//! [`MAX_ANNOTATIONS`] worklist items, and a trailing `::notice` that counts
//! the rest. The comparison annotates only what got worse. Everything else
//! lives in the job summary and the HTML report.

use std::io::{self, Write};

use super::summary::{Summary, WorkItem, WorkKind, count, summarize, summarize_comparison};
use super::{Source, Style};
use crate::report::{Comparison, Location, Report};

/// GitHub shows only the first 10 annotations of each level per step, so
/// emitting more only hides the rest behind a fold.
const MAX_ANNOTATIONS: usize = 10;

/// Other locations named in a copied-code annotation.
const ALSO_LOCATIONS: usize = 3;

/// How a workflow-command string is escaped. Property values also escape the
/// two characters that delimit properties.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Escape {
    Message,
    Property,
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
/// Properties are omitted when absent (no location, no title, a zero line).
fn command(
    out: &mut dyn Write,
    level: &str,
    at: Option<&Location>,
    title: &str,
    message: &str,
) -> io::Result<()> {
    let mut properties = Vec::new();
    if let Some(at) = at {
        properties.push(format!("file={}", escape(&at.path, Escape::Property)));
        if at.start.line > 0 {
            properties.push(format!("line={}", at.start.line));
        }

        if at.end.line > 0 {
            properties.push(format!("endLine={}", at.end.line));
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

/// Writes the report as workflow commands: the verdict, then annotations for
/// the first worklist items.
///
/// # Errors
///
/// Returns an I/O error when writing fails.
pub fn report(
    report: &Report,
    source: &Source,
    _style: Style,
    out: &mut dyn Write,
) -> io::Result<()> {
    let summary = summarize(report, source.name());

    command(out, "notice", None, "deburr", &verdict_message(&summary))?;

    for item in summary.worklist.iter().take(MAX_ANNOTATIONS) {
        // A finding's own message says more than its rule and severity.
        let mut message = if item.kind == WorkKind::Finding {
            item.title.clone()
        } else {
            item.why.clone()
        };
        if let Some(advice) = &item.advice {
            message.push(' ');
            message.push_str(advice);
        }

        if item.kind == WorkKind::Copied && item.locations.len() > 1 {
            let others: Vec<String> = item.locations[1..]
                .iter()
                .take(ALSO_LOCATIONS)
                .map(|at| format!("{}:{}", at.path, at.start.line))
                .collect();
            let more = item.locations.len() - 1 - others.len();

            message.push_str(&format!(" Also at {}", others.join(", ")));
            if more > 0 {
                message.push_str(&format!(" and {more} more"));
            }

            message.push('.');
        }

        command(
            out,
            level_of(item),
            item.locations.first(),
            &format!("deburr: {}", item.short_title),
            &message,
        )?;
    }

    let remaining = summary.worklist.len().saturating_sub(MAX_ANNOTATIONS);
    if remaining > 0 {
        command(
            out,
            "notice",
            None,
            "deburr",
            &format!(
                "{} more {} not annotated. See the job summary or the HTML report for the full list.",
                count(u32::try_from(remaining).unwrap_or(u32::MAX)),
                if remaining == 1 { "item" } else { "items" },
            ),
        )?;
    }

    Ok(())
}

/// Writes the comparison as workflow commands: the verdict, then one warning
/// per entry that got worse. Improvements are not annotated.
///
/// # Errors
///
/// Returns an I/O error when writing fails.
pub fn comparison(
    comparison: &Comparison,
    _source: &Source,
    _style: Style,
    out: &mut dyn Write,
) -> io::Result<()> {
    let summary = summarize_comparison(comparison);

    let mut verdict = summary.verdict.clone();
    if summary.unmatched > 0 {
        verdict.push_str(&format!(
            ". {} items could not be matched between the reports",
            count(summary.unmatched)
        ));
    }

    command(out, "notice", None, "deburr", &verdict)?;

    for entry in summary.worse.iter().take(MAX_ANNOTATIONS) {
        command(
            out,
            "warning",
            entry.location.as_ref(),
            &format!("deburr: {}", entry.title),
            &format!("{}: {} → {}", entry.title, entry.before, entry.after),
        )?;
    }

    let remaining = summary.worse.len().saturating_sub(MAX_ANNOTATIONS);
    if remaining > 0 {
        command(
            out,
            "notice",
            None,
            "deburr",
            &format!(
                "{} more {} that got worse not annotated. See the job summary for the full list.",
                count(u32::try_from(remaining).unwrap_or(u32::MAX)),
                if remaining == 1 { "item" } else { "items" },
            ),
        )?;
    }

    Ok(())
}

/// The three verdict facts on one line, then setup hints and the settled
/// count on following lines.
fn verdict_message(summary: &Summary) -> String {
    let verdict = &summary.verdict;
    let mut message = format!(
        "{} · {} · {}",
        verdict.complexity.headline, verdict.duplication.headline, verdict.findings.headline
    );

    for hint in &summary.hints {
        message.push('\n');
        message.push_str(hint);
    }

    if summary.settled > 0 {
        message.push_str(&format!(
            "\n{} items settled in review.",
            count(summary.settled)
        ));
    }

    message
}

/// Findings keep their own severity; everything else is a warning.
fn level_of(item: &WorkItem) -> &'static str {
    if item.kind != WorkKind::Finding {
        return "warning";
    }

    let severity = item
        .facts
        .iter()
        .find(|fact| fact.key == "Severity")
        .map_or("", |fact| fact.value.as_str());

    match severity.to_lowercase().as_str() {
        "error" => "error",
        "notice" => "notice",
        _ => "warning",
    }
}
