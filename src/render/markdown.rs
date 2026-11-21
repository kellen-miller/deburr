//! GitHub-flavored markdown over the shared summary, for job summaries and
//! pull-request text.
//!
//! The layout lives in `templates/report.md` and `templates/comparison.md`.
//! Report text reaches them only through the `md` filter, so escaping has one
//! home; link fallback and truncation counts stay here as page methods.

use std::io::{self, Write};

use askama::Template;

use super::filters::{self, escape};
use super::summary::{
    ChangeEntry, ComparisonSummary, DirectoryLoad, Fact, Summary, WorkItem, WorkKind, summarize,
    summarize_comparison,
};
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

#[derive(Template)]
#[template(path = "report.md")]
struct ReportPage<'a> {
    summary: Summary,
    report: &'a Report,
    source: &'a Source,
}

impl ReportPage<'_> {
    fn verdict(&self) -> [(&'static str, &Fact); 3] {
        let verdict = &self.summary.verdict;

        [
            ("Complexity", &verdict.complexity),
            ("Duplication", &verdict.duplication),
            ("Findings", &verdict.findings),
        ]
    }

    /// The items in the visible table.
    fn shown(&self) -> &[WorkItem] {
        &self.summary.worklist[..self.summary.worklist.len().min(SHOWN)]
    }

    /// The items folded into the `<details>`.
    fn rest(&self) -> &[WorkItem] {
        self.summary.worklist.get(SHOWN..).unwrap_or_default()
    }

    fn rest_heading(&self) -> String {
        let rest = self.rest().len();

        format!(
            "{} more {}",
            super::summary::count(u32::try_from(rest).unwrap_or(u32::MAX)),
            if rest == 1 { "item" } else { "items" }
        )
    }

    fn directories(&self) -> &[DirectoryLoad] {
        &self.summary.concentration[..self.summary.concentration.len().min(DIRECTORIES)]
    }

    fn kind_word(&self, kind: &WorkKind) -> &'static str {
        kind_word(*kind)
    }

    fn places(&self, locations: &[Location]) -> String {
        places(self.source, locations)
    }
}

#[derive(Template)]
#[template(path = "comparison.md")]
struct ComparisonPage<'a> {
    summary: ComparisonSummary,
    source: &'a Source,
}

impl ComparisonPage<'_> {
    fn shown<'e>(&self, entries: &'e [ChangeEntry]) -> &'e [ChangeEntry] {
        &entries[..entries.len().min(ENTRIES)]
    }

    /// Entries cut from a list.
    fn hidden(&self, entries: &[ChangeEntry]) -> usize {
        entries.len().saturating_sub(ENTRIES)
    }

    fn kind_word(&self, kind: &WorkKind) -> &'static str {
        kind_word(*kind)
    }

    fn place(&self, location: &Location) -> String {
        place(self.source, location)
    }
}

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
    let page = ReportPage {
        summary: summarize(report, source.name()),
        report,
        source,
    };

    out.write_all(page.render().map_err(io::Error::other)?.as_bytes())
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
    let page = ComparisonPage {
        summary: summarize_comparison(comparison),
        source,
    };

    out.write_all(page.render().map_err(io::Error::other)?.as_bytes())
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
