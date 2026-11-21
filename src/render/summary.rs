//! The one human summary every human-facing format renders.
//!
//! `summarize` turns a report into a verdict, setup hints, a ranked worklist,
//! clone groups, and directory concentration; `summarize_comparison` does the
//! same for a comparison. This module owns every policy decision about what a
//! person sees: ranking, grouping, hint thresholds, advice, review filtering,
//! and how numbers read. Renderers only lay the result out.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::report::{
    Category, ChangeState, CloneEndpoint, CodeClone, Comparison, Detail, DuplicationStatus,
    FileStatus, Finding, Function, Location, Position, Report, ReviewKind, ReviewStatus,
};

/// Directory names that usually hold tooling or test helpers rather than
/// shipped code. Matched against every directory component of a path.
const TOOLING_DIRECTORIES: [&str; 9] = [
    "hack",
    "test",
    "tests",
    "e2e",
    "tools",
    "scripts",
    "examples",
    "benchmarks",
    "testutil",
];

/// Most hint paths and concentration rows named.
const NAMED_PATHS: usize = 3;
const CONCENTRATION_ROWS: usize = 10;

/// What a person should see about one report.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    /// Audited directory name, or `report`.
    pub name: String,
    /// Short (7 characters) commit, or empty.
    pub commit: String,
    pub verdict: Verdict,
    pub hints: Vec<String>,
    /// Complete; renderers truncate.
    pub worklist: Vec<WorkItem>,
    /// Retained and refactored items hidden from the worklist.
    pub settled: u32,
    pub groups: Vec<CloneGroup>,
    pub concentration: Vec<DirectoryLoad>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub complexity: Fact,
    pub duplication: Fact,
    pub findings: Fact,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    pub state: FactState,
    pub headline: String,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactState {
    Clear,
    Attention,
    NotMeasured,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkKind {
    Finding,
    Complex,
    Copied,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorkItem {
    pub kind: WorkKind,
    pub category: Category,
    /// Lines involved: function `sloc`, group `copied_lines`, 0 for findings.
    pub size: u32,
    pub title: String,
    /// For one-line text and annotations.
    pub short_title: String,
    pub why: String,
    pub advice: Option<String>,
    pub facts: Vec<Detail>,
    pub locations: Vec<Location>,
    pub deferred: bool,
}

/// Clone pairs that share an owning function, collapsed into one group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloneGroup {
    pub category: Category,
    pub title: String,
    pub copied_lines: u32,
    /// Distinct member function IDs, ordered by path then start line.
    pub functions: Vec<String>,
    /// Clone IDs, in report order.
    pub clones: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryLoad {
    pub directory: String,
    pub code_lines: u32,
    /// Complexity weight, rounded to an integer.
    pub weight: u32,
    pub over_limit: u32,
    pub copied_lines: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComparisonSummary {
    pub verdict: String,
    /// False when the reports cannot support a conclusion.
    pub conclusive: bool,
    pub worse: Vec<ChangeEntry>,
    pub better: Vec<ChangeEntry>,
    /// Items that could not be matched between the reports.
    pub unmatched: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeEntry {
    pub kind: WorkKind,
    pub title: String,
    /// In words, such as `cyclomatic 9`.
    pub before: String,
    pub after: String,
    pub location: Option<Location>,
}

/// `8219` as `8,219`.
#[must_use]
pub fn count(value: u32) -> String {
    let digits = value.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);

    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }

        grouped.push(digit);
    }

    grouped
}

/// A 0..=100 percentage with one decimal: `3.3%`.
#[must_use]
pub fn percent(value: f64) -> String {
    format!("{value:.1}%")
}

/// A float rounded to a whole number; negative and non-finite values are 0.
fn whole(value: f64) -> u32 {
    value.round() as u32
}

fn number_word(value: u32) -> String {
    const WORDS: [&str; 13] = [
        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
        "eleven", "twelve",
    ];

    usize::try_from(value)
        .ok()
        .and_then(|index| WORDS.get(index))
        .map_or_else(|| count(value), |word| (*word).to_owned())
}

fn directory_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(directory, _)| directory)
}

/// `a/b/`, or `./` at the root.
fn directory_label(directory: &str) -> String {
    if directory.is_empty() {
        "./".to_owned()
    } else {
        format!("{directory}/")
    }
}

fn is_tooling_path(path: &str) -> bool {
    directory_of(path)
        .split('/')
        .any(|part| TOOLING_DIRECTORIES.contains(&part))
}

/// "a/", "a/ and b/", "a/, b/ and c/".
fn join_names(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [only] => only.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

/// The identity part of a function ID: everything after the last `::`.
fn bare_name(id: &str) -> &str {
    id.rsplit_once("::").map_or(id, |(_, name)| name)
}

/// Distinct covered lines per path for `(path, start, end)` spans, counting
/// each line once.
fn covered_lines<'a>(
    spans: impl IntoIterator<Item = (&'a str, u32, u32)>,
) -> BTreeMap<&'a str, u32> {
    let mut by_path: BTreeMap<&str, Vec<(u32, u32)>> = BTreeMap::new();

    for (path, start, end) in spans {
        by_path.entry(path).or_default().push((start, end));
    }

    by_path
        .into_iter()
        .map(|(path, mut spans)| {
            spans.sort_unstable();

            let mut total = 0;
            let mut reached = 0;

            for (start, end) in spans {
                let first = start.max(reached + 1);

                if end >= first {
                    total += end - first + 1;
                    reached = end;
                }
            }

            (path, total)
        })
        .collect()
}

/// Union-find over owner indexes.
#[derive(Default)]
struct Owners {
    keys: HashMap<String, usize>,
    parent: Vec<usize>,
}

impl Owners {
    fn index(&mut self, key: String) -> usize {
        if let Some(&index) = self.keys.get(&key) {
            return index;
        }

        let index = self.parent.len();
        self.parent.push(index);
        self.keys.insert(key, index);

        index
    }

    fn find(&mut self, mut index: usize) -> usize {
        while self.parent[index] != index {
            self.parent[index] = self.parent[self.parent[index]];
            index = self.parent[index];
        }

        index
    }

    fn join(&mut self, left: usize, right: usize) {
        let (left, right) = (self.find(left), self.find(right));

        if left != right {
            self.parent[left.max(right)] = left.min(right);
        }
    }
}

/// A function that owns copied code.
#[derive(Debug, Clone)]
struct Member {
    id: String,
    path: String,
    line: u32,
}

impl Member {
    fn name(&self) -> &str {
        bare_name(&self.id)
    }
}

/// One connected set of clones and the functions that own them.
struct Group<'a> {
    category: Category,
    clones: Vec<&'a CodeClone>,
    members: Vec<Member>,
    /// Distinct endpoint regions, by path and position.
    regions: Vec<Location>,
    copied_lines: u32,
}

impl Group<'_> {
    /// A short title: `A / B`, `Name and N variants`, `First and N others`.
    fn title(&self) -> String {
        match self.members.as_slice() {
            [] => self.file_title(),
            [only] => self.single_title(only),
            [first, second] => format!("{} / {}", first.name(), second.name()),
            members => match self.majority_name() {
                Some(name) => format!("{name} and {} variants", members.len() - 1),
                None => format!("{} and {} others", members[0].name(), members.len() - 1),
            },
        }
    }

    /// A sentence-form title for the worklist.
    fn sentence(&self) -> String {
        match self.members.as_slice() {
            [] => self.file_title(),
            [only] => self.single_title(only),
            [first, second] => format!("{} and {} are near copies", first.name(), second.name()),
            members => match self.majority_name() {
                Some(name) => format!("{name} is copied into {} variants", members.len()),
                None => format!(
                    "{} and {} others share copied code",
                    members[0].name(),
                    members.len() - 1
                ),
            },
        }
    }

    fn single_title(&self, member: &Member) -> String {
        format!(
            "{} repeats one block {} times inside itself",
            member.name(),
            self.regions.len()
        )
    }

    fn file_title(&self) -> String {
        let paths: BTreeSet<&str> = self
            .regions
            .iter()
            .map(|region| region.path.as_str())
            .collect();
        let first = paths.first().copied().unwrap_or_default();

        match paths.len() {
            0 | 1 => format!("Copied code in {first}"),
            2 => format!("Copied code in {first} and 1 other file"),
            more => format!("Copied code in {first} and {} other files", more - 1),
        }
    }

    /// The most frequent bare name, when it covers at least half the members.
    fn majority_name(&self) -> Option<&str> {
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();

        for member in &self.members {
            *counts.entry(member.name()).or_default() += 1;
        }

        counts
            .into_iter()
            .max_by_key(|&(name, seen)| (seen, Reverse(name)))
            .filter(|&(_, seen)| seen * 2 >= self.members.len())
            .map(|(name, _)| name)
    }
}

type FunctionIndex<'a> = HashMap<&'a str, Vec<&'a Function>>;

/// The functions an endpoint belongs to: its outermost listed functions.
/// Without an index (comparisons carry no function list), every listed ID.
fn endpoint_owners(
    endpoint: &CloneEndpoint,
    index: Option<&FunctionIndex<'_>>,
) -> Vec<(String, Option<Member>)> {
    if endpoint.functions.is_empty() {
        return vec![(format!("file:{}", endpoint.path), None)];
    }

    let unresolved = |id: &String| {
        let member = Member {
            id: id.clone(),
            path: endpoint.path.clone(),
            line: endpoint.start.line,
        };

        (id.clone(), Some(member))
    };
    let Some(index) = index else {
        return endpoint.functions.iter().map(unresolved).collect();
    };

    let mut resolved: Vec<&Function> = Vec::new();
    let mut owners = Vec::new();

    for id in &endpoint.functions {
        let Some(candidates) = index.get(id.as_str()) else {
            owners.push(unresolved(id));

            continue;
        };
        let overlapping = candidates.iter().find(|function| {
            function.path == endpoint.path
                && function.start <= endpoint.end
                && function.end >= endpoint.start
        });

        resolved.extend(overlapping.or(candidates.first()));
    }

    for function in &resolved {
        let contained = resolved.iter().any(|outer| {
            outer.id != function.id
                && outer.path == function.path
                && outer.start <= function.start
                && function.end <= outer.end
                && (outer.start, outer.end) != (function.start, function.end)
        });

        if !contained {
            let member = Member {
                id: function.id.clone(),
                path: function.path.clone(),
                line: function.start.line,
            };

            owners.push((function.id.clone(), Some(member)));
        }
    }

    owners
}

/// Groups `clones` by shared owners: a union-find over every owner of both
/// endpoints of every clone.
fn group_clones<'a>(clones: &[&'a CodeClone], index: Option<&FunctionIndex<'_>>) -> Vec<Group<'a>> {
    let mut owners = Owners::default();
    let mut members: HashMap<usize, Member> = HashMap::new();
    let mut anchors = Vec::with_capacity(clones.len());

    for clone in clones {
        let mut anchor = None;

        for endpoint in &clone.endpoints {
            for (key, member) in endpoint_owners(endpoint, index) {
                let owner = owners.index(key);

                if let Some(member) = member {
                    members.entry(owner).or_insert(member);
                }

                match anchor {
                    Some(anchor) => owners.join(anchor, owner),
                    None => anchor = Some(owner),
                }
            }
        }

        anchors.push(anchor.unwrap_or_default());
    }

    let mut components: BTreeMap<usize, Vec<&CodeClone>> = BTreeMap::new();

    for (clone, anchor) in clones.iter().zip(anchors) {
        components
            .entry(owners.find(anchor))
            .or_default()
            .push(*clone);
    }

    let mut grouped_members: HashMap<usize, Vec<Member>> = HashMap::new();

    for (owner, member) in members {
        grouped_members
            .entry(owners.find(owner))
            .or_default()
            .push(member);
    }

    components
        .into_iter()
        .map(|(root, clones)| {
            let mut members = grouped_members.remove(&root).unwrap_or_default();
            members.sort_by(|left, right| {
                (&left.path, left.line, &left.id).cmp(&(&right.path, right.line, &right.id))
            });

            let regions: BTreeSet<(&str, Position, Position)> = clones
                .iter()
                .flat_map(|clone| &clone.endpoints)
                .map(|endpoint| (endpoint.path.as_str(), endpoint.start, endpoint.end))
                .collect();
            let copied_lines = covered_lines(
                regions
                    .iter()
                    .map(|&(path, start, end)| (path, start.line, end.line)),
            )
            .values()
            .sum();
            let category = if clones
                .iter()
                .any(|clone| clone.category == Category::Production)
            {
                Category::Production
            } else {
                clones[0].category
            };

            Group {
                category,
                regions: regions
                    .into_iter()
                    .map(|(path, start, end)| Location {
                        path: path.to_owned(),
                        start,
                        end,
                    })
                    .collect(),
                clones,
                members,
                copied_lines,
            }
        })
        .collect()
}

fn function_advice(function: &Function, threshold: u32) -> &'static str {
    let signals = &function.structural_signals;

    if function.max_nesting >= 4 {
        "Flatten it: turn the outer conditions into early returns or continues."
    } else if signals.nested_branches >= 5 {
        "Pull the inner branching into its own function."
    } else if signals.flat_guards >= 4 {
        "Many guard clauses in a row; a lookup table may read better."
    } else if signals.field_mappings >= 8 {
        "Mostly field-by-field mapping; it may be fine as it is."
    } else if function.category == Category::Test && signals.assertion_like_calls >= 8 {
        "Split it into table-driven cases."
    } else if function.cyclomatic <= threshold + 2 {
        "Just over the limit; leave it unless it grows."
    } else {
        "Split it into smaller functions with one job each."
    }
}

fn group_advice(group: &Group<'_>) -> &'static str {
    let paths: BTreeSet<&str> = group
        .regions
        .iter()
        .map(|region| region.path.as_str())
        .collect();
    let directories: BTreeSet<&str> = paths.iter().map(|path| directory_of(path)).collect();

    if group.members.len() == 1 {
        "Loop or table-drive the repeated block."
    } else if group.category == Category::Test {
        "Use a table-driven test or a shared fixture."
    } else if paths.len() == 1 {
        "Extract one shared function in this file."
    } else if directories.len() == 1 {
        "Extract one shared function in this package."
    } else {
        "Keep one copy in a shared package and call it from the others."
    }
}

fn finding_advice(rule: &str) -> Option<&'static str> {
    match rule {
        "redundant-boolean-return" => Some("Return the condition itself."),
        "duplicate-branches" => Some("Merge the identical branches."),
        _ => None,
    }
}

fn location_of(function: &Function) -> Location {
    Location {
        path: function.path.clone(),
        start: function.start,
        end: function.end,
    }
}

fn cyclomatic_words(function: &Function) -> String {
    format!("cyclomatic {}", function.cyclomatic)
}

fn verdict(report: &Report) -> Verdict {
    let threshold = report.config.high_complexity_threshold;
    let over: Vec<&Function> = report
        .functions
        .iter()
        .filter(|function| function.high_complexity)
        .collect();
    let in_category = |category| {
        over.iter()
            .filter(|function| function.category == category)
            .count()
    };
    let total = u32::try_from(over.len()).unwrap_or(u32::MAX);
    let production = u32::try_from(in_category(Category::Production)).unwrap_or(u32::MAX);
    let test = u32::try_from(in_category(Category::Test)).unwrap_or(u32::MAX);

    let complexity = if total == 0 {
        Fact {
            state: FactState::Clear,
            headline: "No function is over the complexity limit".to_owned(),
            detail: format!("Limit: cyclomatic complexity {threshold}."),
        }
    } else {
        let mut detail = if production == total {
            format!("All {} are in production code", number_word(total))
        } else if production == 0 {
            "None are in production code".to_owned()
        } else {
            format!("{} are in production code", number_word(production))
        };

        if production > 0
            && let Some(share) = report.metrics.production.high_complexity_mass_share
        {
            detail.push_str(&format!(
                " and hold {} of its complexity",
                percent(share * 100.0)
            ));
        }

        detail.push('.');

        if test == 0 {
            detail.push_str(" No test function is over.");
        } else if test != total {
            detail.push_str(&format!(" {} are in test code.", number_word(test)));
        }

        Fact {
            state: FactState::Attention,
            headline: format!(
                "{} {} over the complexity limit",
                count(total),
                if total == 1 {
                    "function is"
                } else {
                    "functions are"
                }
            ),
            detail,
        }
    };

    let duplication = &report.duplication;
    let copied = match duplication.status {
        DuplicationStatus::NotRequested => Fact {
            state: FactState::NotMeasured,
            headline: "Duplication was not measured".to_owned(),
            detail: "Clone detection was skipped (--no-duplicates).".to_owned(),
        },
        DuplicationStatus::Error => Fact {
            state: FactState::NotMeasured,
            headline: "Duplication was not measured".to_owned(),
            detail: if duplication.error.is_empty() {
                "Clone detection failed.".to_owned()
            } else {
                duplication.error.clone()
            },
        },
        DuplicationStatus::Measured => {
            let described: Vec<String> = duplication
                .categories
                .iter()
                .map(|category| {
                    let label = match category.category {
                        Category::Production => "production code",
                        Category::Test => "tests",
                        other => other.as_str(),
                    };

                    format!(
                        "{} of {label} copied ({} of {} lines)",
                        percent(category.percentage),
                        count(category.duplicated_code_lines),
                        count(category.code_lines)
                    )
                })
                .collect();
            let any = duplication
                .categories
                .iter()
                .any(|category| category.duplicated_code_lines > 0);

            Fact {
                state: if any {
                    FactState::Attention
                } else {
                    FactState::Clear
                },
                headline: described
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "No analyzed code to measure duplication in".to_owned()),
                detail: described
                    .iter()
                    .skip(1)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" · "),
            }
        }
    };

    let mut by_rule: BTreeMap<&str, u32> = BTreeMap::new();

    for finding in &report.findings {
        *by_rule.entry(finding.rule.as_str()).or_default() += 1;
    }

    let findings = if by_rule.is_empty() {
        let rules: Vec<&str> = report
            .config
            .rules
            .iter()
            .filter(|rule| rule.enabled)
            .map(|rule| rule.id.as_str())
            .collect();

        Fact {
            state: FactState::Clear,
            headline: "No findings".to_owned(),
            detail: if rules.is_empty() {
                "No rules were enabled.".to_owned()
            } else {
                format!("Rules run: {}.", rules.join(", "))
            },
        }
    } else {
        let total = u32::try_from(report.findings.len()).unwrap_or(u32::MAX);

        Fact {
            state: FactState::Attention,
            headline: format!(
                "{} {}",
                count(total),
                if total == 1 { "finding" } else { "findings" }
            ),
            detail: by_rule
                .iter()
                .map(|(rule, seen)| format!("{rule} ×{seen}"))
                .collect::<Vec<_>>()
                .join(", "),
        }
    };

    Verdict {
        complexity,
        duplication: copied,
        findings,
    }
}

/// Parse and read failures, then category skew.
fn hints(report: &Report, production_lines: &BTreeMap<&str, u32>) -> Vec<String> {
    let mut hints = Vec::new();

    let failed: Vec<&str> = report
        .files
        .iter()
        .filter(|file| matches!(file.status, FileStatus::ParseError | FileStatus::ReadError))
        .map(|file| file.path.as_str())
        .collect();

    if !failed.is_empty() {
        let only_parse = report
            .files
            .iter()
            .all(|file| file.status != FileStatus::ReadError);
        let named: Vec<&str> = failed.iter().take(NAMED_PATHS).copied().collect();
        let mut hint = format!(
            "{} {} could not be {}: {}",
            count(u32::try_from(failed.len()).unwrap_or(u32::MAX)),
            if failed.len() == 1 { "file" } else { "files" },
            if only_parse {
                "parsed"
            } else {
                "read or parsed"
            },
            named.join(", ")
        );

        if failed.len() > named.len() {
            hint.push_str(&format!(" and {} more", failed.len() - named.len()));
        }

        hint.push('.');
        hints.push(hint);
    }

    let over: Vec<&Function> = report
        .functions
        .iter()
        .filter(|function| function.high_complexity && function.category == Category::Production)
        .collect();
    let matched: Vec<&&Function> = over
        .iter()
        .filter(|function| is_tooling_path(&function.path))
        .collect();
    let total_lines: u32 = production_lines.values().sum();
    let matched_lines: u32 = production_lines
        .iter()
        .filter(|(path, _)| is_tooling_path(path))
        .map(|(_, lines)| lines)
        .sum();

    let function_share = matched.len() * 2 >= over.len() && !matched.is_empty();
    let line_share = u64::from(matched_lines) * 2 >= u64::from(total_lines) && matched_lines > 0;
    let enough =
        matched.len() >= 2 || matched_lines >= report.duplication.min_lines.saturating_mul(2);

    if (function_share || line_share) && enough {
        let mut weight: BTreeMap<&str, u32> = BTreeMap::new();

        for function in &matched {
            *weight.entry(directory_of(&function.path)).or_default() += 1;
        }

        for (path, lines) in production_lines {
            if is_tooling_path(path) {
                *weight.entry(directory_of(path)).or_default() += lines;
            }
        }

        let mut ranked: Vec<(&str, u32)> = weight.into_iter().collect();
        ranked.sort_by_key(|&(directory, seen)| (Reverse(seen), directory));

        let names: Vec<String> = ranked
            .iter()
            .take(NAMED_PATHS)
            .map(|(directory, _)| directory_label(directory))
            .collect();
        let mut shares = Vec::new();

        if !matched.is_empty() {
            shares.push(format!(
                "{} of the {} over-limit functions",
                matched.len(),
                over.len()
            ));
        }

        if matched_lines > 0 {
            shares.push(format!(
                "{:.0}% of copied production lines",
                f64::from(matched_lines) * 100.0 / f64::from(total_lines)
            ));
        }

        hints.push(format!(
            "{} are in {}. If those are tooling or test helpers, marking them as tests would focus this report on shipped code.",
            shares.join(" and "),
            join_names(&names)
        ));
    }

    hints
}

/// Production directories ranked by complexity weight.
fn concentration(report: &Report, production_lines: &BTreeMap<&str, u32>) -> Vec<DirectoryLoad> {
    let mut loads: BTreeMap<&str, (u32, f64, u32, u32)> = BTreeMap::new();

    for file in &report.files {
        if file.category == Category::Production && file.status == FileStatus::Analyzed {
            loads.entry(directory_of(&file.path)).or_default().0 += file.code_lines;
        }
    }

    for function in &report.functions {
        if function.category == Category::Production {
            let load = loads.entry(directory_of(&function.path)).or_default();
            load.1 += function.mass;
            load.2 += u32::from(function.high_complexity);
        }
    }

    for (path, lines) in production_lines {
        loads.entry(directory_of(path)).or_default().3 += lines;
    }

    let mut loads: Vec<DirectoryLoad> = loads
        .into_iter()
        .map(
            |(directory, (code_lines, mass, over_limit, copied_lines))| DirectoryLoad {
                directory: directory_label(directory),
                code_lines,
                weight: whole(mass),
                over_limit,
                copied_lines,
            },
        )
        .filter(|load| load.weight > 0 || load.copied_lines > 0)
        .collect();

    loads.sort_by(|left, right| {
        right
            .weight
            .cmp(&left.weight)
            .then_with(|| right.copied_lines.cmp(&left.copied_lines))
            .then_with(|| left.directory.cmp(&right.directory))
    });
    loads.truncate(CONCENTRATION_ROWS);

    loads
}

/// Computes the human summary of `report`. `name` is the audited directory's
/// name, or `report`.
#[must_use]
pub fn summarize(report: &Report, name: &str) -> Summary {
    let threshold = report.config.high_complexity_threshold;
    let statuses: HashMap<(ReviewKind, &str), ReviewStatus> = report
        .review
        .iter()
        .flat_map(|ledger| &ledger.items)
        .map(|item| ((item.kind, item.id.as_str()), item.status))
        .collect();
    let status_of = |kind, id: &str| {
        statuses
            .get(&(kind, id))
            .copied()
            .unwrap_or(ReviewStatus::Unreviewed)
    };
    let settled_status =
        |status| matches!(status, ReviewStatus::Retained | ReviewStatus::Refactored);

    let mut settled = 0;
    let mut items: Vec<WorkItem> = Vec::new();
    let file_categories: HashMap<&str, Category> = report
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.category))
        .collect();

    for finding in &report.findings {
        let status = status_of(ReviewKind::Finding, &finding.id);

        if settled_status(status) {
            settled += 1;

            continue;
        }

        items.push(finding_item(finding, &file_categories, status));
    }

    for function in report
        .functions
        .iter()
        .filter(|function| function.high_complexity)
    {
        let status = status_of(ReviewKind::Function, &function.id);

        if settled_status(status) {
            settled += 1;

            continue;
        }

        items.push(function_item(function, threshold, status));
    }

    let mut index: FunctionIndex<'_> = HashMap::new();

    for function in &report.functions {
        index
            .entry(function.id.as_str())
            .or_default()
            .push(function);
    }

    let clones: Vec<&CodeClone> = report.duplication.clones.iter().collect();
    let mut groups = Vec::new();

    for group in group_clones(&clones, Some(&index)) {
        let statuses: Vec<ReviewStatus> = group
            .clones
            .iter()
            .map(|clone| status_of(ReviewKind::Clone, &clone.id))
            .collect();

        if statuses.iter().all(|&status| settled_status(status)) {
            settled += 1;
        } else {
            items.push(group_item(
                &group,
                statuses.contains(&ReviewStatus::Deferred),
            ));
        }

        groups.push(CloneGroup {
            category: group.category,
            title: group.title(),
            copied_lines: group.copied_lines,
            functions: group
                .members
                .iter()
                .map(|member| member.id.clone())
                .collect(),
            clones: group.clones.iter().map(|clone| clone.id.clone()).collect(),
        });
    }

    let rank = |item: &WorkItem| match (item.kind, item.category) {
        (WorkKind::Finding, _) => 0,
        (_, Category::Production) => 1,
        (_, Category::Test) => 2,
        _ => 3,
    };
    let place = |item: &WorkItem| {
        item.locations
            .first()
            .map(|location| (location.path.clone(), location.start))
            .unwrap_or_default()
    };

    items.sort_by_cached_key(|item| {
        (
            rank(item),
            Reverse(item.size),
            place(item),
            item.title.clone(),
        )
    });

    let production_lines = covered_lines(
        report
            .duplication
            .clones
            .iter()
            .filter(|clone| clone.category == Category::Production)
            .flat_map(|clone| &clone.endpoints)
            .map(|endpoint| {
                (
                    endpoint.path.as_str(),
                    endpoint.start.line,
                    endpoint.end.line,
                )
            }),
    );

    Summary {
        name: name.to_owned(),
        commit: report.provenance.source.commit.chars().take(7).collect(),
        verdict: verdict(report),
        hints: hints(report, &production_lines),
        worklist: items,
        settled,
        groups,
        concentration: concentration(report, &production_lines),
    }
}

fn finding_item(
    finding: &Finding,
    file_categories: &HashMap<&str, Category>,
    status: ReviewStatus,
) -> WorkItem {
    let mut facts = vec![
        Detail {
            key: "Rule".to_owned(),
            value: finding.rule.clone(),
        },
        Detail {
            key: "Severity".to_owned(),
            value: finding.severity.clone(),
        },
    ];

    facts.extend(finding.details.iter().cloned());

    WorkItem {
        kind: WorkKind::Finding,
        category: file_categories
            .get(finding.path.as_str())
            .copied()
            .unwrap_or(Category::Production),
        size: 0,
        title: finding.message.clone(),
        short_title: finding.rule.clone(),
        why: format!("Rule {} ({}).", finding.rule, finding.severity),
        advice: finding_advice(&finding.rule).map(str::to_owned),
        facts,
        locations: vec![Location {
            path: finding.path.clone(),
            start: finding.start,
            end: finding.end,
        }],
        deferred: status == ReviewStatus::Deferred,
    }
}

fn function_item(function: &Function, threshold: u32, status: ReviewStatus) -> WorkItem {
    let nesting = if function.max_nesting == 0 {
        String::new()
    } else {
        format!(" and nests {} deep", function.max_nesting)
    };

    WorkItem {
        kind: WorkKind::Complex,
        category: function.category,
        size: function.sloc,
        title: format!(
            "{} has {} paths{nesting}",
            function.name, function.cyclomatic
        ),
        short_title: function.name.clone(),
        why: format!(
            "Cyclomatic complexity {} (limit {threshold}) over {} lines; complexity weight {}.",
            function.cyclomatic,
            count(function.sloc),
            count(whole(function.mass))
        ),
        advice: Some(function_advice(function, threshold).to_owned()),
        facts: vec![
            Detail {
                key: "Cyclomatic".to_owned(),
                value: format!("{} (limit {threshold})", function.cyclomatic),
            },
            Detail {
                key: "Lines".to_owned(),
                value: count(function.sloc),
            },
            Detail {
                key: "Nesting".to_owned(),
                value: function.max_nesting.to_string(),
            },
            Detail {
                key: "Complexity weight".to_owned(),
                value: count(whole(function.mass)),
            },
        ],
        locations: vec![location_of(function)],
        deferred: status == ReviewStatus::Deferred,
    }
}

fn group_item(group: &Group<'_>, deferred: bool) -> WorkItem {
    let places = group.regions.len();

    WorkItem {
        kind: WorkKind::Copied,
        category: group.category,
        size: group.copied_lines,
        title: group.sentence(),
        short_title: group.title(),
        why: format!(
            "{} lines are copied across {places} places.",
            count(group.copied_lines)
        ),
        advice: Some(group_advice(group).to_owned()),
        facts: vec![
            Detail {
                key: "Copied lines".to_owned(),
                value: count(group.copied_lines),
            },
            Detail {
                key: "Places".to_owned(),
                value: places.to_string(),
            },
            Detail {
                key: "Functions".to_owned(),
                value: group.members.len().to_string(),
            },
        ],
        locations: group.regions.clone(),
        deferred,
    }
}

/// Computes the human summary of `comparison`.
#[must_use]
pub fn summarize_comparison(comparison: &Comparison) -> ComparisonSummary {
    let unmatched = comparison
        .function_changes
        .iter()
        .map(|change| change.state)
        .chain(comparison.clone_changes.iter().map(|change| change.state))
        .chain(comparison.finding_changes.iter().map(|change| change.state))
        .filter(|state| {
            matches!(
                state,
                ChangeState::Ambiguous | ChangeState::Incompatible | ChangeState::Incomplete
            )
        })
        .count();
    let unmatched = u32::try_from(unmatched).unwrap_or(u32::MAX);

    if !comparison.compatible || !comparison.complete {
        let reason = if !comparison.reason.is_empty() {
            comparison.reason.clone()
        } else if !comparison.compatible {
            "the reports are not comparable".to_owned()
        } else {
            "the reports are incomplete".to_owned()
        };

        return ComparisonSummary {
            verdict: format!("No conclusion: {reason}"),
            conclusive: false,
            worse: Vec::new(),
            better: Vec::new(),
            unmatched,
        };
    }

    let mut worse = Vec::new();
    let mut better = Vec::new();

    for finding in &comparison.new_findings {
        worse.push(finding_entry(finding, "none", &finding.rule));
    }

    for finding in &comparison.resolved_findings {
        better.push(finding_entry(finding, &finding.rule, "gone"));
    }

    for change in &comparison.function_changes {
        let function_entry = |before: &str, after: &str, function: &Function| ChangeEntry {
            kind: WorkKind::Complex,
            title: function.name.clone(),
            before: before.to_owned(),
            after: after.to_owned(),
            location: Some(location_of(function)),
        };

        match (change.state, &change.before, &change.after) {
            (ChangeState::New, _, Some(after)) if after.high_complexity => {
                worse.push(function_entry(
                    "not present",
                    &cyclomatic_words(after),
                    after,
                ));
            }
            (ChangeState::CrossedThreshold, Some(before), Some(after)) => {
                worse.push(function_entry(
                    &cyclomatic_words(before),
                    &cyclomatic_words(after),
                    after,
                ));
            }
            (ChangeState::Matched, Some(before), Some(after))
                if before.high_complexity
                    && after.high_complexity
                    && after.cyclomatic > before.cyclomatic =>
            {
                worse.push(function_entry(
                    &cyclomatic_words(before),
                    &cyclomatic_words(after),
                    after,
                ));
            }
            (ChangeState::ResolvedThreshold, Some(before), Some(after)) => {
                better.push(function_entry(
                    &cyclomatic_words(before),
                    &cyclomatic_words(after),
                    after,
                ));
            }
            (ChangeState::DeletedFunction | ChangeState::SourceRemoved, Some(before), _)
                if before.high_complexity =>
            {
                better.push(function_entry(&cyclomatic_words(before), "removed", before));
            }
            _ => {}
        }
    }

    let added: Vec<&CodeClone> = comparison
        .clone_changes
        .iter()
        .filter(|change| change.state == ChangeState::New)
        .filter_map(|change| change.after.as_ref())
        .collect();
    let removed: Vec<&CodeClone> = comparison
        .clone_changes
        .iter()
        .filter(|change| change.state == ChangeState::Removed)
        .filter_map(|change| change.before.as_ref())
        .collect();

    for group in group_clones(&added, None) {
        let lines = format!("{} copied lines", count(group.copied_lines));

        worse.push(group_entry(&group, "not present", &lines));
    }

    for group in group_clones(&removed, None) {
        let lines = format!("{} copied lines", count(group.copied_lines));

        better.push(group_entry(&group, &lines, "gone"));
    }

    ComparisonSummary {
        verdict: format!("{} got worse, {} improved", worse.len(), better.len()),
        conclusive: true,
        worse,
        better,
        unmatched,
    }
}

fn finding_entry(finding: &Finding, before: &str, after: &str) -> ChangeEntry {
    ChangeEntry {
        kind: WorkKind::Finding,
        title: finding.message.clone(),
        before: before.to_owned(),
        after: after.to_owned(),
        location: Some(Location {
            path: finding.path.clone(),
            start: finding.start,
            end: finding.end,
        }),
    }
}

fn group_entry(group: &Group<'_>, before: &str, after: &str) -> ChangeEntry {
    ChangeEntry {
        kind: WorkKind::Copied,
        title: group.title(),
        before: before.to_owned(),
        after: after.to_owned(),
        location: group.regions.first().cloned(),
    }
}
