//! The HTML report page (`templates/report.html`).

use std::cmp::Reverse;
use std::collections::{BTreeSet, HashMap};
use std::io::{self, Write};

use askama::Template;

use super::code::{Code, Place};
use super::{CSS, SOURCE_ITEMS, clone_minimum, embedded_json, working_tree};
use crate::render::summary::{
    CloneGroup, DirectoryLoad, Fact, FactState, WorkItem, WorkKind, count, summarize,
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

struct Tile {
    tone: &'static str,
    label: &'static str,
    headline: String,
    detail: String,
}

struct Card {
    tone: &'static str,
    label: &'static str,
    test: bool,
    deferred: bool,
    title: String,
    why: String,
    advice: Option<String>,
    facts: Vec<Detail>,
    places: Vec<Place>,
    more_places: usize,
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

struct DirectoryRow {
    directory: String,
    code_lines: String,
    weight: String,
    weight_bar: u32,
    over_limit: u32,
    copied: String,
    copied_bar: u32,
}

struct ComplexRow {
    name: String,
    test: bool,
    bar: u32,
    cyclomatic: u32,
    lines: String,
    nesting: u32,
    place: Place,
}

struct GroupRow {
    tone: &'static str,
    label: String,
    title: String,
    copied_lines: String,
    function_count: usize,
    places: Vec<Place>,
    more_places: usize,
    functions: Vec<String>,
    more_functions: usize,
}

struct FunctionRow {
    name: String,
    weight: String,
    cyclomatic: u32,
    over: bool,
    lines: String,
    nesting: u32,
    place: Place,
}

struct AuditGroup {
    title: &'static str,
    rows: Vec<Detail>,
}

#[derive(Template)]
#[template(path = "report.html")]
struct ReportPage {
    css: &'static str,
    js: &'static str,
    json: String,
    name: String,
    meta: Vec<String>,
    nothing_analyzed: bool,
    tiles: Vec<Tile>,
    hints: Vec<String>,
    items: Vec<Card>,
    source_capped: bool,
    source_cap: usize,
    settled_count: u32,
    settled: Vec<SettledRow>,
    group_count: usize,
    complex_total: usize,
    concentration: Vec<DirectoryRow>,
    limit: u32,
    limit_bar: u32,
    complex: Vec<ComplexRow>,
    complex_hidden: usize,
    production_groups: usize,
    test_groups: usize,
    groups: Vec<GroupRow>,
    groups_hidden: usize,
    function_total: String,
    link_prefix: String,
    functions: Vec<FunctionRow>,
    audit: Vec<AuditGroup>,
}

/// `value` as a percentage of `max`, at most 100.
fn bar(value: u32, max: u32) -> u32 {
    if max == 0 {
        return 0;
    }

    u32::try_from(u64::from(value) * 100 / u64::from(max)).map_or(100, |share| share.min(100))
}

/// A complexity weight as a whole number with separators.
fn weight(mass: f64) -> String {
    format!("{:.0}", mass.max(0.0))
        .parse()
        .map_or_else(|_| "0".to_owned(), count)
}

fn tile(label: &'static str, fact: &Fact, attention: &'static str) -> Tile {
    Tile {
        tone: match fact.state {
            FactState::Clear => "ok",
            FactState::Attention => attention,
            FactState::NotMeasured => "off",
        },
        label,
        headline: fact.headline.clone(),
        detail: fact.detail.clone(),
    }
}

/// One worklist item as a card. Source is read only when `with_source`.
fn card(item: &WorkItem, with_source: bool, source: &Source) -> Card {
    let (tone, label) = match item.kind {
        WorkKind::Finding => ("bad", "Finding"),
        WorkKind::Complex => ("cx", "Complex"),
        WorkKind::Copied => ("dup", "Copied code"),
    };
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
        tone,
        label,
        test: item.category == Category::Test,
        deferred: item.deferred,
        title: item.title.clone(),
        why: item.why.clone(),
        advice: item.advice.clone(),
        facts: item.facts.clone(),
        places: item
            .locations
            .iter()
            .take(NAMED)
            .map(|location| Place::new(source, location))
            .collect(),
        more_places: item.locations.len().saturating_sub(NAMED),
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

fn directory_rows(concentration: &[DirectoryLoad]) -> Vec<DirectoryRow> {
    let heaviest = concentration
        .iter()
        .map(|load| load.weight)
        .max()
        .unwrap_or(0);
    let most_copied = concentration
        .iter()
        .map(|load| load.copied_lines)
        .max()
        .unwrap_or(0);

    concentration
        .iter()
        .map(|load| DirectoryRow {
            directory: load.directory.clone(),
            code_lines: count(load.code_lines),
            weight: count(load.weight),
            weight_bar: bar(load.weight, heaviest),
            over_limit: load.over_limit,
            copied: count(load.copied_lines),
            copied_bar: bar(load.copied_lines, most_copied),
        })
        .collect()
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

    let items: Vec<Card> = summary
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
    let complex: Vec<ComplexRow> = over
        .iter()
        .take(LISTED_ROWS)
        .map(|function| ComplexRow {
            name: function.name.clone(),
            test: function.category == Category::Test,
            bar: bar(function.cyclomatic, scale),
            cyclomatic: function.cyclomatic,
            lines: count(function.sloc),
            nesting: function.max_nesting,
            place: Place::new(
                source,
                &Location {
                    path: function.path.clone(),
                    start: function.start,
                    end: function.end,
                },
            ),
        })
        .collect();

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
        .map(|group| {
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
            let (tone, label) = match group.category {
                Category::Production => ("dup", "Production".to_owned()),
                Category::Test => ("t", "Test".to_owned()),
                other => ("t", other.as_str().to_owned()),
            };

            GroupRow {
                tone,
                label,
                title: group.title.clone(),
                copied_lines: count(group.copied_lines),
                function_count: group.functions.len(),
                places: regions
                    .iter()
                    .take(NAMED)
                    .map(|&(path, start, end)| Place {
                        text: format!("{path}:{start}"),
                        href: source.link(path, start, end),
                    })
                    .collect(),
                more_places: regions.len().saturating_sub(NAMED),
                functions: group
                    .functions
                    .iter()
                    .take(NAMED)
                    .map(|id| {
                        id.rsplit_once("::")
                            .map_or(id.as_str(), |(_, name)| name)
                            .to_owned()
                    })
                    .collect(),
                more_functions: group.functions.len().saturating_sub(NAMED),
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

    let limit_bar = bar(threshold, scale);
    let page = ReportPage {
        css: CSS,
        js: JS,
        json: embedded_json(report)?,
        name: summary.name.clone(),
        meta,
        nothing_analyzed: coverage.analyzed == 0,
        tiles: vec![
            tile("Complexity", &summary.verdict.complexity, "cx"),
            tile("Duplication", &summary.verdict.duplication, "dup"),
            tile("Findings", &summary.verdict.findings, "bad"),
        ],
        hints: summary.hints.clone(),
        source_capped: summary.worklist.len() > SOURCE_ITEMS,
        source_cap: SOURCE_ITEMS,
        settled_count: summary.settled,
        settled: settled_rows(report, &summary.groups, source),
        items,
        group_count: summary.groups.len(),
        complex_total: over.len(),
        concentration: directory_rows(&summary.concentration),
        limit: threshold,
        limit_bar,
        complex_hidden: over.len().saturating_sub(complex.len()),
        complex,
        production_groups: summary
            .groups
            .iter()
            .filter(|group| group.category == Category::Production)
            .count(),
        test_groups: summary
            .groups
            .iter()
            .filter(|group| group.category == Category::Test)
            .count(),
        groups_hidden: summary.groups.len().saturating_sub(groups.len()),
        groups,
        function_total: count(u32::try_from(every.len()).unwrap_or(u32::MAX)),
        link_prefix: source
            .link(PATH_MARKER, 1, 1)
            .and_then(|link| {
                link.split_once(PATH_MARKER)
                    .map(|(prefix, _)| prefix.to_owned())
            })
            .unwrap_or_default(),
        functions: every
            .iter()
            .take(STATIC_FUNCTION_ROWS)
            .map(|function| FunctionRow {
                name: function.name.clone(),
                weight: weight(function.mass),
                cyclomatic: function.cyclomatic,
                over: function.high_complexity,
                lines: count(function.sloc),
                nesting: function.max_nesting,
                place: Place::new(
                    source,
                    &Location {
                        path: function.path.clone(),
                        start: function.start,
                        end: function.end,
                    },
                ),
            })
            .collect(),
        audit: audit_details(report),
    };

    out.write_all(page.render().map_err(io::Error::other)?.as_bytes())
}
