//! The HTML comparison page (`templates/comparison.html`).

use std::io::{self, Write};

use askama::Template;

use super::code::{Code, Place};
use super::{CSS, SOURCE_ITEMS, clone_minimum, embedded_json, working_tree};
use crate::render::summary::{ChangeEntry, WorkKind, count, summarize_comparison};
use crate::render::{Source, Style, enum_name};
use crate::report::{
    AnalyzerIdentity, Comparison, ConfigSummary, Coverage, Detail, DuplicationStatus, Provenance,
};

/// Entry results that mean the code no longer exists in the newer tree, so
/// there is no source to show. These are the words `summarize_comparison`
/// uses for removed functions, resolved findings, and removed clones.
const GONE: [&str; 2] = ["gone", "removed"];

struct EntryRow {
    tone: &'static str,
    label: &'static str,
    title: String,
    before: String,
    after: String,
    place: Option<Place>,
    block: Option<Code>,
}

struct Side {
    title: &'static str,
    rows: Vec<Detail>,
}

#[derive(Template)]
#[template(path = "comparison.html")]
struct ComparisonPage {
    css: &'static str,
    json: String,
    name: String,
    before_commit: String,
    after_commit: String,
    tone: &'static str,
    verdict: String,
    conclusive: bool,
    worse: Vec<EntryRow>,
    better: Vec<EntryRow>,
    source_capped: bool,
    source_cap: usize,
    unmatched: u32,
    unmatched_sentence: String,
    sides: Vec<Side>,
}

/// One comparison entry. `shown` is how many entries already carry source.
fn entry_row(entry: &ChangeEntry, shown: usize, source: &Source) -> EntryRow {
    let (tone, label) = match entry.kind {
        WorkKind::Finding => ("bad", "Finding"),
        WorkKind::Complex => ("cx", "Complex"),
        WorkKind::Copied => ("dup", "Copied code"),
    };
    let present = !GONE.contains(&entry.after.as_str());

    EntryRow {
        tone,
        label,
        title: entry.title.clone(),
        before: entry.before.clone(),
        after: entry.after.clone(),
        place: entry
            .location
            .as_ref()
            .map(|location| Place::new(source, location)),
        block: entry
            .location
            .as_ref()
            .filter(|_| present && shown < SOURCE_ITEMS)
            .and_then(|location| Code::read(source, location)),
    }
}

/// One side of the audit details.
fn side(
    title: &'static str,
    provenance: &Provenance,
    analyzer: &AnalyzerIdentity,
    config: &ConfigSummary,
    coverage: &Coverage,
    duplication: DuplicationStatus,
) -> Side {
    let detail = |key: &str, value: String| Detail {
        key: key.to_owned(),
        value,
    };
    let commit = &provenance.source.commit;

    Side {
        title,
        rows: vec![
            detail(
                "Commit",
                if commit.is_empty() {
                    "unknown".to_owned()
                } else {
                    commit.clone()
                },
            ),
            detail("Working tree", working_tree(&provenance.source).to_owned()),
            detail("Analyzer", format!("{} {}", analyzer.id, analyzer.version)),
            detail(
                "Complexity limit",
                format!("cyclomatic {}", config.high_complexity_threshold),
            ),
            detail("Clone minimum", clone_minimum(&config.duplication)),
            detail("Files analyzed", count(coverage.analyzed)),
            detail("Duplication", enum_name(&duplication)),
        ],
    }
}

/// Writes the HTML comparison.
///
/// # Errors
///
/// Returns an I/O error when rendering or writing fails.
pub fn comparison(
    comparison: &Comparison,
    source: &Source,
    _style: Style,
    out: &mut dyn Write,
) -> io::Result<()> {
    let summary = summarize_comparison(comparison);
    let worse: Vec<EntryRow> = summary
        .worse
        .iter()
        .enumerate()
        .map(|(index, entry)| entry_row(entry, index, source))
        .collect();
    let better: Vec<EntryRow> = summary
        .better
        .iter()
        .enumerate()
        .map(|(index, entry)| entry_row(entry, summary.worse.len() + index, source))
        .collect();
    let short = |provenance: &Provenance| {
        let commit: String = provenance.source.commit.chars().take(7).collect();

        if commit.is_empty() {
            "unknown".to_owned()
        } else {
            commit
        }
    };

    let page = ComparisonPage {
        css: CSS,
        json: embedded_json(comparison)?,
        name: source.name().to_owned(),
        before_commit: short(&comparison.before_provenance),
        after_commit: short(&comparison.after_provenance),
        tone: if !summary.conclusive {
            "off"
        } else if summary.worse.is_empty() {
            "ok"
        } else {
            "cx"
        },
        verdict: summary.verdict.clone(),
        conclusive: summary.conclusive,
        source_capped: summary.worse.len() + summary.better.len() > SOURCE_ITEMS,
        source_cap: SOURCE_ITEMS,
        unmatched: summary.unmatched,
        unmatched_sentence: format!(
            "{} {} could not be matched between the reports.",
            count(summary.unmatched),
            if summary.unmatched == 1 {
                "item"
            } else {
                "items"
            }
        ),
        worse,
        better,
        sides: vec![
            side(
                "Before",
                &comparison.before_provenance,
                &comparison.before_analyzer,
                &comparison.before_config,
                &comparison.before_coverage,
                comparison.duplication.before_status,
            ),
            side(
                "After",
                &comparison.after_provenance,
                &comparison.after_analyzer,
                &comparison.after_config,
                &comparison.after_coverage,
                comparison.duplication.after_status,
            ),
        ],
    };

    out.write_all(page.render().map_err(io::Error::other)?.as_bytes())
}
