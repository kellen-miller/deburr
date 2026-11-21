//! GitHub-flavored markdown over the shared summary, for job summaries and
//! pull-request text.

use std::io::{self, Write};

use super::summary::{ChangeEntry, Fact, WorkItem, count, summarize, summarize_comparison};
use super::text::kind_word;
use super::{Source, Style};
use crate::report::{Category, Comparison, Location, Report};

/// Items in the visible "Start here" table; the rest sit in a `<details>`.
const SHOWN: usize = 10;

/// Directories in the concentration table.
const DIRECTORIES: usize = 5;

/// Locations named per cell before `+N more`.
const PLACES: usize = 3;

/// Rows per comparison list before `… N more`.
const ENTRIES: usize = 25;

/// Writes the markdown report.
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

    writeln!(out, "## deburr · {}\n", escape(&summary.name))?;

    writeln!(out, "| | |\n|---|---|")?;
    let verdict = &summary.verdict;
    for (label, fact) in [
        ("Complexity", &verdict.complexity),
        ("Duplication", &verdict.duplication),
        ("Findings", &verdict.findings),
    ] {
        writeln!(out, "| **{label}** | {} |", fact_cell(fact))?;
    }

    writeln!(out)?;
    for hint in &summary.hints {
        writeln!(out, "> {}\n", escape(hint))?;
    }

    if summary.worklist.is_empty() {
        writeln!(out, "No open items.")?;
    } else {
        writeln!(out, "### Start here\n")?;
        item_table(
            out,
            &summary.worklist[..summary.worklist.len().min(SHOWN)],
            1,
            source,
        )?;
    }

    if summary.worklist.len() > SHOWN {
        let rest = &summary.worklist[SHOWN..];

        writeln!(
            out,
            "\n<details><summary>{} more {}</summary>\n",
            count(u32::try_from(rest.len()).unwrap_or(u32::MAX)),
            if rest.len() == 1 { "item" } else { "items" }
        )?;
        item_table(out, rest, SHOWN + 1, source)?;
        writeln!(out, "\n</details>")?;
    }

    if summary.settled > 0 {
        writeln!(
            out,
            "\n{} {} settled in review.",
            count(summary.settled),
            if summary.settled == 1 {
                "item"
            } else {
                "items"
            }
        )?;
    }

    if !summary.concentration.is_empty() {
        writeln!(
            out,
            "\n### Where it concentrates\n\n| Directory | Complexity weight | Over limit | Copied lines | Code lines |\n|---|--:|--:|--:|--:|"
        )?;
        for load in summary.concentration.iter().take(DIRECTORIES) {
            writeln!(
                out,
                "| {} | {} | {} | {} | {} |",
                escape(&load.directory),
                count(load.weight),
                count(load.over_limit),
                count(load.copied_lines),
                count(load.code_lines)
            )?;
        }
    }

    write!(out, "\n---\ndeburr {}", escape(&report.analyzer.version))?;
    if !summary.commit.is_empty() {
        write!(out, " · {}", escape(&summary.commit))?;
    }

    writeln!(
        out,
        " <!-- deburr report schema {} -->",
        report.schema_version
    )
}

/// Writes the markdown comparison.
///
/// # Errors
///
/// Returns an I/O error when writing fails.
pub fn comparison(
    comparison: &Comparison,
    source: &Source,
    _style: Style,
    out: &mut dyn Write,
) -> io::Result<()> {
    let summary = summarize_comparison(comparison);

    writeln!(out, "### This pull request\n")?;
    writeln!(out, "**{}**", escape(&summary.verdict))?;

    let lists = [("🔺", &summary.worse), ("✅", &summary.better)];

    if lists.iter().any(|(_, entries)| !entries.is_empty()) {
        writeln!(out, "\n| | Item | Before | After |\n|---|---|---|---|")?;
    }

    for (marker, entries) in lists {
        for entry in entries.iter().take(ENTRIES) {
            writeln!(out, "{}", entry_row(marker, entry, source))?;
        }
    }

    for (marker, entries) in lists {
        if entries.len() > ENTRIES {
            writeln!(out, "\n{marker} … {} more", entries.len() - ENTRIES)?;
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

/// Escapes text taken from a report so it cannot form markdown, HTML, or a
/// table cell boundary. Line breaks become spaces.
fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());

    for character in text.chars() {
        match character {
            '|' | '<' | '>' | '`' | '*' | '\\' | '[' | ']' => {
                escaped.push('\\');
                escaped.push(character);
            }
            '\n' | '\r' => escaped.push(' '),
            other => escaped.push(other),
        }
    }

    escaped
}

fn fact_cell(fact: &Fact) -> String {
    if fact.detail.is_empty() {
        escape(&fact.headline)
    } else {
        format!("{} — {}", escape(&fact.headline), escape(&fact.detail))
    }
}

/// `path:line` as a permalink when the source has one, otherwise as code.
fn place(source: &Source, location: &Location) -> String {
    let text = format!("{}:{}", location.path, location.start.line);

    match source.link(&location.path, location.start.line, location.end.line) {
        Some(link) => format!("[{}]({link})", escape(&text)),
        None if text.contains('`') => escape(&text),
        None => format!("`{text}`"),
    }
}

fn places(source: &Source, locations: &[Location]) -> String {
    let mut cell: Vec<String> = locations
        .iter()
        .take(PLACES)
        .map(|location| place(source, location))
        .collect();

    if locations.len() > PLACES {
        cell.push(format!("+{} more", locations.len() - PLACES));
    }

    cell.join("<br>")
}

/// One table of worklist items, numbered from `first`.
fn item_table(
    out: &mut dyn Write,
    items: &[WorkItem],
    first: usize,
    source: &Source,
) -> io::Result<()> {
    writeln!(out, "| # | Item | Why | Where |\n|--:|---|---|---|")?;

    for (offset, item) in items.iter().enumerate() {
        let mut title = format!("**{}** {}", kind_word(item.kind), escape(&item.title));

        if item.category == Category::Test {
            title.push_str(" _(test)_");
        }

        if item.deferred {
            title.push_str(" _(deferred)_");
        }

        let mut why = escape(&item.why);
        if let Some(advice) = &item.advice {
            why.push_str(&format!("<br>{}", escape(advice)));
        }

        writeln!(
            out,
            "| {} | {title} | {why} | {} |",
            first + offset,
            places(source, &item.locations)
        )?;
    }

    Ok(())
}

fn entry_row(marker: &str, entry: &ChangeEntry, source: &Source) -> String {
    let mut item = format!("**{}** {}", kind_word(entry.kind), escape(&entry.title));

    if let Some(location) = &entry.location {
        item.push_str(&format!("<br>{}", place(source, location)));
    }

    format!(
        "| {marker} | {item} | {} | {} |",
        escape(&entry.before),
        escape(&entry.after)
    )
}
