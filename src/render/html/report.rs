//! The HTML report page (`templates/report.html`).

use std::cmp::Reverse;
use std::collections::{BTreeSet, HashMap};
use std::io::{self, Write};

use askama::Template;

use super::code::{Code, Place};
use super::{CSS, SOURCE_ITEMS, clone_minimum, embedded_json, working_tree};
use crate::render::filters;
use crate::render::summary::{
    CloneGroup, DirectoryLoad, FactState, Summary, WorkItem, WorkKind, count, summarize,
};
use crate::render::{Source, Style, enum_name};
use crate::report::{
    Category, CodeClone, Coverage, Detail, Function, Location, Report, ReviewItem, ReviewKind,
    ReviewStatus,
};

/// The report's own script: the function filter and the copy button.
const JS: &str = include_str!("../../../templates/report.js");

/// Rows written as HTML in "All functions"; the script builds the rest from
/// the embedded JSON.
const STATIC_FUNCTION_ROWS: usize = 50;

/// Most rows listed in "Complex functions" and "Copied code".
const LISTED_ROWS: usize = 500;

/// Most places and member names named on one card or group.
const NAMED: usize = 6;

/// A stand-in for a path, so the script can build links from one prefix.
const PATH_MARKER: &str = "DEBURRPATH";

/// A worklist item with the source blocks read for it.
struct Card<'a> {
    item: &'a WorkItem,
    blocks_label: String,
    blocks: Vec<Code>,
    /// Source was wanted for this card but could not be read.
    unavailable: bool,
}

struct SettledRow {
    status: String,
    label: &'static str,
    title: String,
    place: Option<Place>,
    reason: String,
}

/// A copied-code group with the regions its clones cover.
struct GroupRow<'a> {
    group: &'a CloneGroup,
    places: Vec<Place>,
    more_places: usize,
}

struct AuditGroup {
    title: &'static str,
    rows: Vec<Detail>,
}

#[derive(Template)]
#[template(path = "report.html")]
struct ReportPage<'a> {
    css: &'static str,
    js: &'static str,
    json: String,
    report: &'a Report,
    source: &'a Source,
    summary: &'a Summary,
    meta: Vec<String>,
    cards: Vec<Card<'a>>,
    settled_rows: Vec<SettledRow>,
    /// Functions over the complexity limit, worst first.
    over: Vec<&'a Function>,
    /// Cyclomatic complexity at which a bar is full.
    scale: u32,
    groups: Vec<GroupRow<'a>>,
    /// Every function, heaviest first.
    every: Vec<&'a Function>,
    link_prefix: String,
    audit: Vec<AuditGroup>,
}

impl ReportPage<'_> {
    /// The tile color for a fact: `attention` when it needs a look.
    fn tone<'t>(&self, state: &FactState, attention: &'t str) -> &'t str {
        match state {
            FactState::Clear => "ok",
            FactState::Attention => attention,
            FactState::NotMeasured => "off",
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

    fn source_capped(&self) -> bool {
        self.summary.worklist.len() > SOURCE_ITEMS
    }

    fn source_cap(&self) -> usize {
        SOURCE_ITEMS
    }

    /// The first places of a card, linked when the source has a link base.
    fn places(&self, locations: &[Location]) -> Vec<Place> {
        locations
            .iter()
            .take(NAMED)
            .map(|location| Place::new(self.source, location))
            .collect()
    }

    /// How many of `total` places or names are left out of the list.
    fn more(&self, total: usize) -> usize {
        total.saturating_sub(NAMED)
    }

    fn function_place(&self, function: &Function) -> Place {
        Place::new(
            self.source,
            &Location {
                path: function.path.clone(),
                start: function.start,
                end: function.end,
            },
        )
    }

    fn bar(&self, value: &u32, max: &u32) -> u32 {
        bar(*value, *max)
    }

    fn weight_bar(&self, load: &DirectoryLoad) -> u32 {
        let heaviest = self.summary.concentration.iter().map(|load| load.weight);

        bar(load.weight, heaviest.max().unwrap_or(0))
    }

    fn copied_bar(&self, load: &DirectoryLoad) -> u32 {
        let most = self
            .summary
            .concentration
            .iter()
            .map(|load| load.copied_lines);

        bar(load.copied_lines, most.max().unwrap_or(0))
    }

    /// The complexity limit as a position on a complexity bar.
    fn limit_bar(&self) -> u32 {
        bar(self.report.config.high_complexity_threshold, self.scale)
    }

    /// The largest over-limit functions, as many as the page lists.
    fn complex(&self) -> &[&Function] {
        &self.over[..self.over.len().min(LISTED_ROWS)]
    }

    fn complex_hidden(&self) -> usize {
        self.over.len() - self.complex().len()
    }

    fn groups_in(&self, category: Category) -> usize {
        self.summary
            .groups
            .iter()
            .filter(|group| group.category == category)
            .count()
    }

    fn group_tone(&self, group: &CloneGroup) -> &'static str {
        match group.category {
            Category::Production => "dup",
            _ => "t",
        }
    }

    fn group_label(&self, group: &CloneGroup) -> String {
        match group.category {
            Category::Production => "Production".to_owned(),
            Category::Test => "Test".to_owned(),
            other => other.as_str().to_owned(),
        }
    }

    /// The first member functions of a group, by bare name.
    fn function_names<'g>(&self, group: &'g CloneGroup) -> Vec<&'g str> {
        group
            .functions
            .iter()
            .take(NAMED)
            .map(|id| id.rsplit_once("::").map_or(id.as_str(), |(_, name)| name))
            .collect()
    }

    fn function_total(&self) -> String {
        count(u32::try_from(self.every.len()).unwrap_or(u32::MAX))
    }

    /// The heaviest functions, written as HTML rows.
    fn listed_functions(&self) -> &[&Function] {
        &self.every[..self.every.len().min(STATIC_FUNCTION_ROWS)]
    }
}

/// `value` as a percentage of `max`, at most 100.
fn bar(value: u32, max: u32) -> u32 {
    if max == 0 {
        return 0;
    }

    u32::try_from(u64::from(value) * 100 / u64::from(max)).map_or(100, |share| share.min(100))
}

/// One worklist item as a card. Source is read only when `with_source`.
fn card<'a>(item: &'a WorkItem, with_source: bool, source: &Source) -> Card<'a> {
    let mut blocks = Vec::new();
    let mut blocks_label = String::new();

    if with_source && item.kind == WorkKind::Copied && item.locations.len() > 1 {
        // The two largest copies, side by side.
        let mut by_size: Vec<&Location> = item.locations.iter().collect();
        by_size
            .sort_by_key(|location| Reverse(location.end.line.saturating_sub(location.start.line)));

        let pair: Option<Vec<Code>> = by_size[..2]
            .iter()
            .map(|location| Code::read(source, location))
            .collect();

        if let Some(mut pair) = pair
            && let [left, right] = pair.as_mut_slice()
        {
            Code::mark_differences(left, right);
            blocks_label = if item.locations.len() == 2 {
                "Compare the copies".to_owned()
            } else {
                format!("Compare 2 of the {} copies", item.locations.len())
            };
            blocks = pair;
        }
    } else if with_source
        && let Some(location) = item.locations.first()
        && let Some(block) = Code::read(source, location)
    {
        blocks_label = if item.kind == WorkKind::Complex {
            "Show the function".to_owned()
        } else {
            "Show the code".to_owned()
        };
        blocks.push(block);
    }

    Card {
        item,
        unavailable: with_source && blocks.is_empty(),
        blocks_label,
        blocks,
    }
}

/// Items the ledger settled (retained or refactored), which the worklist
/// leaves out. A clone group is settled only when all its clones are.
fn settled_rows(report: &Report, groups: &[CloneGroup], source: &Source) -> Vec<SettledRow> {
    let Some(ledger) = &report.review else {
        return Vec::new();
    };
    let settled = |status| matches!(status, ReviewStatus::Retained | ReviewStatus::Refactored);
    let clones: HashMap<&str, &ReviewItem> = ledger
        .items
        .iter()
        .filter(|item| item.kind == ReviewKind::Clone)
        .map(|item| (item.id.as_str(), item))
        .collect();
    let functions: HashMap<&str, &Function> = report
        .functions
        .iter()
        .rev()
        .map(|function| (function.id.as_str(), function))
        .collect();
    let findings: HashMap<&str, &str> = report
        .findings
        .iter()
        .map(|finding| (finding.id.as_str(), finding.message.as_str()))
        .collect();
    let row = |item: &ReviewItem, label, title: String| SettledRow {
        status: enum_name(&item.status),
        label,
        title,
        place: item
            .locations
            .first()
            .map(|location| Place::new(source, location)),
        reason: item.reason.clone(),
    };

    let mut rows = Vec::new();

    for item in ledger.items.iter().filter(|item| settled(item.status)) {
        match item.kind {
            ReviewKind::Function => {
                if let Some(function) = functions.get(item.id.as_str())
                    && function.high_complexity
                {
                    rows.push(row(item, "Complex", function.name.clone()));
                }
            }
            ReviewKind::Finding => {
                if let Some(message) = findings.get(item.id.as_str()) {
                    rows.push(row(item, "Finding", (*message).to_owned()));
                }
            }
            ReviewKind::Clone => {}
        }
    }

    for group in groups {
        let members: Vec<&ReviewItem> = group
            .clones
            .iter()
            .filter_map(|id| clones.get(id.as_str()).copied())
            .collect();

        if members.len() == group.clones.len()
            && let Some(first) = members.first()
            && members.iter().all(|item| settled(item.status))
        {
            rows.push(row(first, "Copied code", group.title.clone()));
        }
    }

    rows
}

/// Files analyzed in one category.
fn analyzed_in(coverage: &Coverage, category: Category) -> u32 {
    coverage
        .by_category
        .iter()
        .find(|entry| entry.category == category)
        .map_or(0, |entry| entry.analyzed)
}

/// ` (68 production, 53 test)`, or nothing when neither category has files.
fn breakdown(coverage: &Coverage) -> String {
    let production = analyzed_in(coverage, Category::Production);
    let test = analyzed_in(coverage, Category::Test);

    if production + test == 0 {
        return String::new();
    }

    format!(" ({} production, {} test)", count(production), count(test))
}

/// Files, settings, and provenance in words, for the audit details.
fn audit_details(report: &Report) -> Vec<AuditGroup> {
    let detail = |key: &str, value: String| Detail {
        key: key.to_owned(),
        value,
    };
    let coverage = &report.coverage;
    let config = &report.config;
    let source = &report.provenance.source;
    let analyzed = format!("{}{}", count(coverage.analyzed), breakdown(coverage));
    let mut included = Vec::new();
    let mut excluded = Vec::new();

    for (name, wanted) in [
        ("tests", config.include_tests),
        ("testdata", config.include_testdata),
        ("vendor", config.include_vendor),
        ("generated", config.include_generated),
    ] {
        if wanted {
            included.push(name);
        } else {
            excluded.push(name);
        }
    }

    let scope = match (included.is_empty(), excluded.is_empty()) {
        (_, true) => format!("production and {}", included.join(", ")),
        (true, false) => format!("production only · not {}", excluded.join(", ")),
        (false, false) => format!(
            "production and {} · not {}",
            included.join(", "),
            excluded.join(", ")
        ),
    };
    let rules: Vec<&str> = config
        .rules
        .iter()
        .filter(|rule| rule.enabled)
        .map(|rule| rule.id.as_str())
        .collect();
    let languages: Vec<String> = report
        .scope
        .languages
        .iter()
        .map(|language| match language.as_str() {
            "go" => "Go".to_owned(),
            "typescript" => "TypeScript".to_owned(),
            other => other.to_owned(),
        })
        .collect();

    vec![
        AuditGroup {
            title: "Files",
            rows: vec![
                detail("Discovered", count(coverage.discovered)),
                detail("Analyzed", analyzed),
                detail("Excluded", count(coverage.excluded)),
                detail("Not Go or TypeScript", count(coverage.unsupported)),
                detail("Read errors", count(coverage.read_errors)),
                detail("Parse errors", count(coverage.parse_errors)),
                detail("Symlinks skipped", count(coverage.symlinks)),
                detail("Excluded directories", count(coverage.excluded_directories)),
            ],
        },
        AuditGroup {
            title: "Settings",
            rows: vec![
                detail("Languages", languages.join(", ")),
                detail(
                    "Complexity limit",
                    format!("cyclomatic {}", config.high_complexity_threshold),
                ),
                detail("Clone minimum", clone_minimum(&config.duplication)),
                detail("Includes", scope),
                detail(
                    "Rules",
                    if rules.is_empty() {
                        "none enabled".to_owned()
                    } else {
                        rules.join(", ")
                    },
                ),
            ],
        },
        AuditGroup {
            title: "Provenance",
            rows: vec![
                detail(
                    "Analyzer",
                    format!(
                        "{} {}, schema {}",
                        report.analyzer.id, report.analyzer.version, report.schema_version
                    ),
                ),
                detail("Build", report.provenance.build.version.clone()),
                detail(
                    "Commit",
                    if source.commit.is_empty() {
                        "unknown".to_owned()
                    } else {
                        source.commit.clone()
                    },
                ),
                detail("Working tree", working_tree(source).to_owned()),
                detail("Source manifest", source.manifest_digest.clone()),
            ],
        },
    ]
}

/// Writes the HTML report.
///
/// # Errors
///
/// Returns an I/O error when rendering or writing fails.
pub fn report(
    report: &Report,
    source: &Source,
    _style: Style,
    out: &mut dyn Write,
) -> io::Result<()> {
    let summary = summarize(report, source.name());
    let clones: HashMap<&str, &CodeClone> = report
        .duplication
        .clones
        .iter()
        .map(|clone| (clone.id.as_str(), clone))
        .collect();
    let cards: Vec<Card> = summary
        .worklist
        .iter()
        .enumerate()
        .map(|(index, item)| card(item, index < SOURCE_ITEMS, source))
        .collect();

    let threshold = report.config.high_complexity_threshold;
    let mut over: Vec<&Function> = report
        .functions
        .iter()
        .filter(|function| function.high_complexity)
        .collect();
    over.sort_by(|left, right| {
        right
            .cyclomatic
            .cmp(&left.cyclomatic)
            .then_with(|| right.sloc.cmp(&left.sloc))
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.start.cmp(&right.start))
            .then_with(|| left.id.cmp(&right.id))
    });
    let scale = over
        .first()
        .map_or(0, |function| function.cyclomatic)
        .max(threshold.saturating_mul(2))
        .max(1);

    let mut ranked: Vec<&CloneGroup> = summary.groups.iter().collect();
    ranked.sort_by_key(|group| {
        (
            match group.category {
                Category::Production => 0,
                Category::Test => 1,
                _ => 2,
            },
            Reverse(group.copied_lines),
            group.title.as_str(),
        )
    });
    let groups: Vec<GroupRow> = ranked
        .iter()
        .take(LISTED_ROWS)
        .map(|&group| {
            let regions: BTreeSet<(&str, u32, u32)> = group
                .clones
                .iter()
                .filter_map(|id| clones.get(id.as_str()))
                .flat_map(|clone| &clone.endpoints)
                .map(|endpoint| {
                    (
                        endpoint.path.as_str(),
                        endpoint.start.line,
                        endpoint.end.line,
                    )
                })
                .collect();

            GroupRow {
                group,
                places: regions
                    .iter()
                    .take(NAMED)
                    .map(|&(path, start, end)| Place {
                        text: format!("{path}:{start}"),
                        href: source.link(path, start, end),
                    })
                    .collect(),
                more_places: regions.len().saturating_sub(NAMED),
            }
        })
        .collect();

    let mut every: Vec<&Function> = report.functions.iter().collect();
    every.sort_by(|left, right| {
        right
            .mass
            .total_cmp(&left.mass)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.start.cmp(&right.start))
            .then_with(|| left.id.cmp(&right.id))
    });

    let coverage = &report.coverage;
    let skipped: u32 = coverage
        .by_category
        .iter()
        .filter(|entry| matches!(entry.category, Category::Vendor | Category::Generated))
        .map(|entry| entry.excluded)
        .sum();
    let mut meta = Vec::new();

    if !summary.commit.is_empty() {
        meta.push(format!("Commit {}", summary.commit));
    }

    if report.provenance.source.git_known && report.provenance.source.dirty {
        meta.push("uncommitted changes".to_owned());
    }

    meta.push(format!(
        "{} files analyzed{}",
        count(coverage.analyzed),
        breakdown(coverage)
    ));

    if skipped > 0 {
        meta.push(format!(
            "{} vendor and generated files skipped",
            count(skipped)
        ));
    }

    let page = ReportPage {
        css: CSS,
        js: JS,
        json: embedded_json(report)?,
        report,
        source,
        meta,
        cards,
        settled_rows: settled_rows(report, &summary.groups, source),
        over,
        scale,
        groups,
        every,
        link_prefix: source
            .link(PATH_MARKER, 1, 1)
            .and_then(|link| {
                link.split_once(PATH_MARKER)
                    .map(|(prefix, _)| prefix.to_owned())
            })
            .unwrap_or_default(),
        audit: audit_details(report),
        summary: &summary,
    };

    out.write_all(page.render().map_err(io::Error::other)?.as_bytes())
}
