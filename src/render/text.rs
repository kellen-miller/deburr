//! Plain-text rendering over the shared summary, and the embedded cleanup
//! guide.

use std::io::{self, Write};

use super::summary::{
    ChangeEntry, FactState, WorkItem, WorkKind, count, summarize, summarize_comparison,
};
use super::{Source, Style};
use crate::report::{Category, Comparison, Location, Report};

/// Worklist items and change entries shown per list; JSON and HTML carry them
/// all.
const SHOWN: usize = 10;

/// Locations named per item before `+N more`.
const PLACES: usize = 3;

const RED: &str = "31";
const GREEN: &str = "32";
const YELLOW: &str = "33";
const MAGENTA: &str = "35";
const DIM: &str = "2";

const CLEANUP_GUIDE: &str = include_str!("../../docs/cleanup.md");

/// Writes the text report: verdict, hints, and the first items to work on.
///
/// # Errors
///
/// Returns an I/O error when writing fails.
pub fn report(
    report: &Report,
    source: &Source,
    style: Style,
    out: &mut dyn Write,
) -> io::Result<()> {
    let summary = summarize(report, source.name());
    let color = style.color;

    write!(out, "deburr · {}", summary.name)?;
    if !summary.commit.is_empty() {
        write!(out, " @ {}", summary.commit)?;
    }

    write!(out, " · {} files analyzed", count(report.coverage.analyzed))?;
    if report.coverage.excluded > 0 {
        write!(
            out,
            " ({} vendor/generated skipped)",
            count(report.coverage.excluded)
        )?;
    }

    writeln!(out, "\n")?;

    let verdict = &summary.verdict;
    for (label, fact, attention) in [
        ("Complexity", &verdict.complexity, YELLOW),
        ("Duplication", &verdict.duplication, MAGENTA),
        ("Findings", &verdict.findings, RED),
    ] {
        let tint = match fact.state {
            FactState::Clear => GREEN,
            FactState::Attention => attention,
            FactState::NotMeasured => DIM,
        };

        writeln!(
            out,
            "  {} {label:<13}{}",
            paint(color, tint, "●"),
            fact.headline
        )?;
        if !fact.detail.is_empty() {
            writeln!(out, "{:17}{}", "", fact.detail)?;
        }
    }

    if !summary.hints.is_empty() {
        writeln!(out)?;
    }

    for hint in &summary.hints {
        writeln!(out, "  {} {hint}", paint(color, YELLOW, "!"))?;
    }

    writeln!(out)?;
    if summary.worklist.is_empty() {
        writeln!(out, "No open items.")?;
    } else {
        writeln!(out, "Start here")?;
    }

    for (index, item) in summary.worklist.iter().take(SHOWN).enumerate() {
        let word = kind_word(item.kind);
        let tint = match item.kind {
            WorkKind::Finding => RED,
            WorkKind::Complex => YELLOW,
            WorkKind::Copied => MAGENTA,
        };
        let mut tags = String::new();

        if item.category == Category::Test {
            tags.push_str(" [test]");
        }

        if item.deferred {
            tags.push_str(" [deferred]");
        }

        writeln!(
            out,
            "{:>3}  {}{} {}{tags}",
            index + 1,
            paint(color, tint, word),
            " ".repeat(8usize.saturating_sub(word.len())),
            item.title
        )?;

        let indent = " ".repeat(14);
        writeln!(out, "{indent}{}", item.why)?;
        if let Some(advice) = &item.advice {
            writeln!(out, "{indent}{advice}")?;
        }

        writeln!(out, "{indent}{}", places(item))?;
    }

    if summary.worklist.len() > SHOWN {
        writeln!(out, "     … {} more", summary.worklist.len() - SHOWN)?;
    }

    if summary.settled > 0 {
        writeln!(
            out,
            "\n{} {} settled in review",
            count(summary.settled),
            if summary.settled == 1 {
                "item"
            } else {
                "items"
            }
        )?;
    }

    writeln!(
        out,
        "\nFull report:  deburr audit . --format html --output deburr.html"
    )?;
    writeln!(out, "For tools:    deburr audit . --format json")
}

/// Writes the text comparison: the verdict, what got worse, what improved.
///
/// # Errors
///
/// Returns an I/O error when writing fails.
pub fn comparison(
    comparison: &Comparison,
    _source: &Source,
    style: Style,
    out: &mut dyn Write,
) -> io::Result<()> {
    let summary = summarize_comparison(comparison);
    let color = style.color;

    writeln!(out, "deburr · comparison · {}", summary.verdict)?;

    for (heading, tint, entries) in [
        ("Got worse", RED, &summary.worse),
        ("Improved", GREEN, &summary.better),
    ] {
        if entries.is_empty() {
            continue;
        }

        writeln!(out, "\n{}", paint(color, tint, heading))?;
        for entry in entries.iter().take(SHOWN) {
            writeln!(out, "  {}", entry_line(entry, color))?;
        }

        if entries.len() > SHOWN {
            writeln!(out, "  … {} more", entries.len() - SHOWN)?;
        }
    }

    if summary.unmatched > 0 {
        writeln!(
            out,
            "\n{} {} could not be matched between the reports.",
            count(summary.unmatched),
            if summary.unmatched == 1 {
                "item"
            } else {
                "items"
            }
        )?;
    }

    Ok(())
}

/// Writes the cleanup guide printed by `deburr guide cleanup`.
///
/// # Errors
///
/// Returns an I/O error when writing fails.
pub fn cleanup_guide(out: &mut dyn Write) -> io::Result<()> {
    out.write_all(CLEANUP_GUIDE.as_bytes())
}

/// Wraps `text` in an ANSI color only when `color` is on, so piped output
/// never holds an escape byte.
fn paint(color: bool, code: &str, text: &str) -> String {
    if color {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_owned()
    }
}

pub(super) fn kind_word(kind: WorkKind) -> &'static str {
    match kind {
        WorkKind::Finding => "finding",
        WorkKind::Complex => "complex",
        WorkKind::Copied => "copied",
    }
}

/// `path:line` for a location, so terminals make it clickable.
fn place(location: &Location) -> String {
    format!("{}:{}", location.path, location.start.line)
}

/// The item's first few locations, then how many more there are.
fn places(item: &WorkItem) -> String {
    let named: Vec<String> = item.locations.iter().take(PLACES).map(place).collect();
    let mut text = named.join("  ");

    if item.locations.len() > PLACES {
        text.push_str(&format!("  +{} more", item.locations.len() - PLACES));
    }

    text
}

fn entry_line(entry: &ChangeEntry, color: bool) -> String {
    let tint = match entry.kind {
        WorkKind::Finding => RED,
        WorkKind::Complex => YELLOW,
        WorkKind::Copied => MAGENTA,
    };
    let word = kind_word(entry.kind);
    let mut line = format!(
        "{}{} {}  {} → {}",
        paint(color, tint, word),
        " ".repeat(8usize.saturating_sub(word.len())),
        entry.title,
        entry.before,
        entry.after
    );

    if let Some(location) = &entry.location {
        line.push_str(&format!("  {}", place(location)));
    }

    line
}
