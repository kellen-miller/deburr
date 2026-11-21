//! The HTML comparison page (`templates/comparison.html`).

use std::io::{self, Write};

use askama::Template;

use super::code::{Code, Place};
use super::{CSS, SOURCE_ITEMS, clone_minimum, embedded_json, working_tree};
use crate::render::filters;
use crate::render::summary::{
    ChangeEntry, ComparisonSummary, WorkKind, count, summarize_comparison,
};
use crate::render::{Source, Style, enum_name};
use crate::report::{
    AnalyzerIdentity, Comparison, ConfigSummary, Coverage, Detail, DuplicationStatus, Location,
    Provenance,
};

/// Entry results that mean the code no longer exists in the newer tree, so
/// there is no source to show. These are the words `summarize_comparison`
/// uses for removed functions, resolved findings, and removed clones.
const GONE: [&str; 2] = ["gone", "removed"];

/// A change entry with the newer source read for it.
struct Entry<'a> {
    entry: &'a ChangeEntry,
    block: Option<Code>,
}

struct Side {
    title: &'static str,
    rows: Vec<Detail>,
}

#[derive(Template)]
#[template(path = "comparison.html")]
struct ComparisonPage<'a> {
    css: &'static str,
    json: String,
    comparison: &'a Comparison,
    source: &'a Source,
    summary: &'a ComparisonSummary,
    worse: Vec<Entry<'a>>,
    better: Vec<Entry<'a>>,
    sides: Vec<Side>,
}

impl ComparisonPage<'_> {
    /// Seven characters of the commit, or `unknown`.
    fn short_commit(&self, provenance: &Provenance) -> String {
        let commit: String = provenance.source.commit.chars().take(7).collect();

        if commit.is_empty() {
            "unknown".to_owned()
        } else {
            commit
        }
    }

    fn tone(&self) -> &'static str {
        if !self.summary.conclusive {
            "off"
        } else if self.summary.worse.is_empty() {
            "ok"
        } else {
            "cx"
        }
    }

    fn kind_tone(&self, kind: &WorkKind) -> &'static str {
        match kind {
            WorkKind::Finding => "bad",
            WorkKind::Complex => "cx",
            WorkKind::Copied => "dup",
        }
    }

    fn kind_label(&self, kind: &WorkKind) -> &'static str {
        match kind {
            WorkKind::Finding => "Finding",
            WorkKind::Complex => "Complex",
            WorkKind::Copied => "Copied code",
        }
    }

    fn place(&self, location: &Location) -> Place {
        Place::new(self.source, location)
    }

    fn source_capped(&self) -> bool {
        self.summary.worse.len() + self.summary.better.len() > SOURCE_ITEMS
    }

    fn source_cap(&self) -> usize {
        SOURCE_ITEMS
    }
}

/// One comparison entry. `shown` is how many entries already carry source.
fn entry<'a>(entry: &'a ChangeEntry, shown: usize, source: &Source) -> Entry<'a> {
    let present = !GONE.contains(&entry.after.as_str());

    Entry {
        entry,
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
    let worse: Vec<Entry> = summary
        .worse
        .iter()
        .enumerate()
        .map(|(index, item)| entry(item, index, source))
        .collect();
    let better: Vec<Entry> = summary
        .better
        .iter()
        .enumerate()
        .map(|(index, item)| entry(item, summary.worse.len() + index, source))
        .collect();

    let page = ComparisonPage {
        css: CSS,
        json: embedded_json(comparison)?,
        comparison,
        source,
        summary: &summary,
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
