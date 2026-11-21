//! Clone grouping: pairs that share functions collapse into one group, which
//! then becomes one worklist item or comparison entry.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::{Detail, WorkItem, WorkKind, bare_name, count, directory_of};
use crate::report::{Category, CloneEndpoint, CodeClone, Function, Location, Position};

/// Distinct covered lines per path for `(path, start, end)` spans, counting
/// each line once.
pub(super) fn covered_lines<'a>(
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
pub(super) struct Member {
    pub(super) id: String,
    path: String,
    line: u32,
}

impl Member {
    fn name(&self) -> &str {
        bare_name(&self.id)
    }
}

/// One connected set of clones and the functions that own them.
pub(super) struct Group<'a> {
    pub(super) category: Category,
    pub(super) clones: Vec<&'a CodeClone>,
    pub(super) members: Vec<Member>,
    /// Distinct endpoint regions, by path and position.
    pub(super) regions: Vec<Location>,
    pub(super) copied_lines: u32,
}

impl Group<'_> {
    /// A short title: `A / B`, `Name and N variants`, `First and N others`.
    pub(super) fn title(&self) -> String {
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

pub(super) type FunctionIndex<'a> = HashMap<&'a str, Vec<&'a Function>>;

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
pub(super) fn group_clones<'a>(
    clones: &[&'a CodeClone],
    index: Option<&FunctionIndex<'_>>,
) -> Vec<Group<'a>> {
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

pub(super) fn group_advice(group: &Group<'_>) -> &'static str {
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

pub(super) fn group_item(group: &Group<'_>, deferred: bool) -> WorkItem {
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
