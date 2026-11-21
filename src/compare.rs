//! Compares two reports from the same analyzer contract.
//!
//! Findings, functions, and clones are each matched in two passes: first by
//! ID, then by fingerprint among whatever the ID pass left over. A key that
//! appears more than once on either side, or a match involving an
//! identity-ambiguous item, is reported as ambiguous instead of guessed.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::report::{
    AmbiguousMatch, Change, ChangeState, CodeClone, Comparison, Coverage, DuplicationComparison,
    DuplicationStatus, Finding, Function, MatchBasis, MetricBucket, MetricDelta, MetricsDelta,
    Report, ReviewItem, ReviewKind, ReviewLedger, SCHEMA_VERSION,
};
use crate::review;

impl ChangeState {
    /// The serialized name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Matched => "matched",
            Self::New => "new",
            Self::Removed => "removed",
            Self::Ambiguous => "ambiguous",
            Self::NoLongerReported => "no_longer_reported",
            Self::SourceRemoved => "source_removed",
            Self::DeletedFunction => "deleted_function",
            Self::ResolvedThreshold => "resolved_threshold",
            Self::CrossedThreshold => "crossed_threshold",
            Self::Incompatible => "incompatible",
            Self::Incomplete => "incomplete",
        }
    }
}

/// Paths the `after` report still has source for, used to tell a removed
/// file from a removed function or a resolved finding.
struct SourcePaths<'a> {
    known: bool,
    present: HashSet<&'a str>,
}

impl<'a> SourcePaths<'a> {
    fn of(report: &'a Report) -> Self {
        let analyzed = report
            .files
            .iter()
            .filter(|file| file.status == crate::report::FileStatus::Analyzed)
            .map(|file| file.path.as_str());
        let functions = report
            .functions
            .iter()
            .map(|function| function.path.as_str());
        let findings = report.findings.iter().map(|finding| finding.path.as_str());

        Self {
            known: !report.files.is_empty(),
            present: analyzed.chain(functions).chain(findings).collect(),
        }
    }

    fn lacks(&self, path: &str) -> bool {
        self.known && !self.present.contains(path)
    }
}

/// An inventory entry kind (finding, function, clone) that `compare` can
/// match across reports.
trait Tracked: Clone {
    type SortKey: Ord;

    const KIND: ReviewKind;

    fn id(&self) -> &str;

    fn fingerprint(&self) -> &str;

    fn identity_ambiguous(&self) -> bool;

    /// Orders the inventory before matching.
    fn sort_key(&self) -> Self::SortKey;

    /// Orders changes whose `after` (else `before`) is this item.
    fn change_key(&self, state: ChangeState) -> String;

    /// An ambiguity report naming this item, without basis or IDs.
    fn ambiguous_summary(&self) -> AmbiguousMatch;

    fn matched_state(_before: &Self, _after: &Self) -> ChangeState {
        ChangeState::Matched
    }

    /// The state of an item with no counterpart in `after`.
    fn removed_state(&self, after: &SourcePaths<'_>) -> ChangeState;
}

impl Tracked for Finding {
    type SortKey = (String, u32, u32, String, String);

    const KIND: ReviewKind = ReviewKind::Finding;

    fn id(&self) -> &str {
        &self.id
    }

    fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    fn identity_ambiguous(&self) -> bool {
        self.identity_ambiguous
    }

    fn sort_key(&self) -> Self::SortKey {
        (
            self.path.clone(),
            self.start.line,
            self.start.column,
            self.rule.clone(),
            self.id.clone(),
        )
    }

    fn change_key(&self, state: ChangeState) -> String {
        format!(
            "{}:{:09}:{:09}:{}:{}",
            self.path,
            self.start.line,
            self.start.column,
            state.as_str(),
            self.id
        )
    }

    fn ambiguous_summary(&self) -> AmbiguousMatch {
        AmbiguousMatch {
            rule: self.rule.clone(),
            path: self.path.clone(),
            message: self.message.clone(),
            ..ambiguous_match(Self::KIND)
        }
    }

    fn removed_state(&self, after: &SourcePaths<'_>) -> ChangeState {
        if after.lacks(&self.path) {
            ChangeState::SourceRemoved
        } else {
            ChangeState::NoLongerReported
        }
    }
}

impl Tracked for Function {
    type SortKey = String;

    const KIND: ReviewKind = ReviewKind::Function;

    fn id(&self) -> &str {
        &self.id
    }

    fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    fn identity_ambiguous(&self) -> bool {
        self.identity_ambiguous
    }

    fn sort_key(&self) -> Self::SortKey {
        format!(
            "{}:{:09}:{:09}:{}:{}",
            self.path, self.start.line, self.start.column, self.name, self.id
        )
    }

    fn change_key(&self, state: ChangeState) -> String {
        format!(
            "{}:{:09}:{:09}:{}:{}",
            self.path,
            self.start.line,
            self.start.column,
            state.as_str(),
            self.id
        )
    }

    fn ambiguous_summary(&self) -> AmbiguousMatch {
        AmbiguousMatch {
            path: self.path.clone(),
            message: self.name.clone(),
            ..ambiguous_match(Self::KIND)
        }
    }

    fn matched_state(before: &Self, after: &Self) -> ChangeState {
        match (before.high_complexity, after.high_complexity) {
            (true, false) => ChangeState::ResolvedThreshold,
            (false, true) => ChangeState::CrossedThreshold,
            _ => ChangeState::Matched,
        }
    }

    fn removed_state(&self, after: &SourcePaths<'_>) -> ChangeState {
        if after.lacks(&self.path) {
            ChangeState::SourceRemoved
        } else {
            ChangeState::DeletedFunction
        }
    }
}

impl CodeClone {
    /// The first endpoint's location as a fixed-width sortable string.
    fn sort_path(&self) -> String {
        let endpoint = &self.endpoints[0];

        format!(
            "{}:{:09}:{:09}:{:09}:{:09}",
            endpoint.path,
            endpoint.start.line,
            endpoint.start.column,
            endpoint.end.line,
            endpoint.end.column
        )
    }
}

impl Tracked for CodeClone {
    type SortKey = String;

    const KIND: ReviewKind = ReviewKind::Clone;

    fn id(&self) -> &str {
        &self.id
    }

    fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    fn identity_ambiguous(&self) -> bool {
        self.identity_ambiguous
    }

    fn sort_key(&self) -> Self::SortKey {
        format!(
            "{}:{}:{:09}:{:09}:{}",
            self.category.as_str(),
            self.sort_path(),
            self.tokens,
            self.endpoints[0].code_lines,
            self.id
        )
    }

    fn change_key(&self, state: ChangeState) -> String {
        format!("{}:{}:{}", self.sort_path(), state.as_str(), self.id)
    }

    fn ambiguous_summary(&self) -> AmbiguousMatch {
        AmbiguousMatch {
            path: self.sort_path(),
            message: "duplicate source spans".to_owned(),
            ..ambiguous_match(Self::KIND)
        }
    }

    fn removed_state(&self, _after: &SourcePaths<'_>) -> ChangeState {
        ChangeState::Removed
    }
}

fn ambiguous_match(kind: ReviewKind) -> AmbiguousMatch {
    AmbiguousMatch {
        kind: kind.as_str().to_owned(),
        state: Some(ChangeState::Ambiguous),
        match_basis: None,
        rule: String::new(),
        path: String::new(),
        message: String::new(),
        before_ids: Vec::new(),
        after_ids: Vec::new(),
    }
}

fn bare_change<T>(state: ChangeState) -> Change<T> {
    Change {
        state,
        match_basis: None,
        before: None,
        after: None,
        before_review: None,
        after_review: None,
        before_ids: Vec::new(),
        after_ids: Vec::new(),
    }
}

/// Compares `before` and `after`, both already validated.
///
/// The result is incompatible and incomplete, with a `reason` and no metric
/// delta or inventory changes, when the reports were not produced under the
/// same contract (see `compatibility_reason`), when coverage is incomplete,
/// or when a report's attached review ledger is invalid.
#[must_use]
pub fn compare(before: &Report, after: &Report) -> Comparison {
    let (before_review, before_ledger_problem) = ledger_for_compare(before, "before");
    let (after_review, after_ledger_problem) = ledger_for_compare(after, "after");

    let mut result = Comparison {
        metrics_delta: None,
        after_provenance: after.provenance.clone(),
        before_provenance: before.provenance.clone(),
        before_analyzer: before.analyzer.clone(),
        after_analyzer: after.analyzer.clone(),
        schema_version: SCHEMA_VERSION.to_owned(),
        reason: String::new(),
        ambiguous_clones: Vec::new(),
        function_changes: Vec::new(),
        resolved_findings: Vec::new(),
        ambiguous_functions: Vec::new(),
        clone_changes: Vec::new(),
        ambiguous_findings: Vec::new(),
        new_findings: Vec::new(),
        finding_changes: Vec::new(),
        before_scope: before.scope.clone(),
        after_scope: after.scope.clone(),
        duplication: compare_duplication(before, after),
        before_metrics: before.metrics.clone(),
        after_metrics: after.metrics.clone(),
        before_config: before.config.clone(),
        after_config: after.config.clone(),
        before_review,
        after_review,
        before_coverage: before.coverage.clone(),
        after_coverage: after.coverage.clone(),
        complete: true,
        compatible: true,
    };

    let reason = before_ledger_problem
        .or(after_ledger_problem)
        .or_else(|| compatibility_reason(before, after));

    if let Some(reason) = reason {
        result.reason = reason;
        result.compatible = false;
        result.complete = false;

        return result;
    }

    result.metrics_delta = Some(MetricsDelta {
        all: metric_delta(&before.metrics.all, &after.metrics.all),
        production: metric_delta(&before.metrics.production, &after.metrics.production),
        test: metric_delta(&before.metrics.test, &after.metrics.test),
    });

    let after_paths = SourcePaths::of(after);

    (result.finding_changes, result.ambiguous_findings) = compare_inventory(
        &before.findings,
        &after.findings,
        &result.before_review,
        &result.after_review,
        &after_paths,
    );
    (result.function_changes, result.ambiguous_functions) = compare_inventory(
        &before.functions,
        &after.functions,
        &result.before_review,
        &result.after_review,
        &after_paths,
    );
    (result.clone_changes, result.ambiguous_clones) = compare_inventory(
        &before.duplication.clones,
        &after.duplication.clones,
        &result.before_review,
        &result.after_review,
        &after_paths,
    );

    for change in &result.finding_changes {
        match (change.state, &change.before, &change.after) {
            (ChangeState::New, _, Some(finding)) => result.new_findings.push(finding.clone()),
            (
                ChangeState::NoLongerReported | ChangeState::SourceRemoved | ChangeState::Removed,
                Some(finding),
                _,
            ) => result.resolved_findings.push(finding.clone()),
            _ => {}
        }
    }

    result.new_findings.sort_by_cached_key(Tracked::sort_key);
    result
        .resolved_findings
        .sort_by_cached_key(Tracked::sort_key);

    result
}

/// The ledger `compare` shows for `report`, plus a reason to refuse the
/// comparison when the report's attached ledger does not apply to it.
fn ledger_for_compare(report: &Report, side: &str) -> (ReviewLedger, Option<String>) {
    match review::current_ledger(report) {
        Ok(ledger) => (ledger, None),
        Err(error) => (
            review::init_ledger(report),
            Some(format!("{side} review ledger is invalid: {error}")),
        ),
    }
}

/// Why `before` and `after` cannot be compared, in fixed priority order.
fn compatibility_reason(before: &Report, after: &Report) -> Option<String> {
    if before.schema_version != after.schema_version {
        return Some(format!(
            "schema versions differ: {} vs {}",
            before.schema_version, after.schema_version
        ));
    }

    if before.analyzer != after.analyzer {
        return Some("analyzer identity differs".to_owned());
    }

    let before_duplication = &before.config.duplication;
    let after_duplication = &after.config.duplication;

    if before_duplication.requested != after_duplication.requested {
        return Some("duplication request differs".to_owned());
    }

    if !same_duplication_settings(before, after) {
        return Some("duplication settings differ".to_owned());
    }

    let both_measured = before.duplication.status == DuplicationStatus::Measured
        && after.duplication.status == DuplicationStatus::Measured;

    if before_duplication.requested && !both_measured {
        return Some("requested duplication measurement is incomplete".to_owned());
    }

    if before.duplication.status != after.duplication.status {
        return Some("duplication status differs".to_owned());
    }

    if before.config != after.config {
        return Some("configuration differs".to_owned());
    }

    if before.scope != after.scope {
        return Some("analysis scope differs".to_owned());
    }

    if coverage_incomplete(&before.coverage) || coverage_incomplete(&after.coverage) {
        return Some(
            "coverage is incomplete; findings and improvement cannot be compared".to_owned(),
        );
    }

    None
}

/// Whether both reports measured clones with the same minimum tokens and
/// lines, in the config summary and in the duplication section.
fn same_duplication_settings(before: &Report, after: &Report) -> bool {
    before.config.duplication.min_tokens == after.config.duplication.min_tokens
        && before.config.duplication.min_lines == after.config.duplication.min_lines
        && before.duplication.min_tokens == after.duplication.min_tokens
        && before.duplication.min_lines == after.duplication.min_lines
}

fn coverage_incomplete(coverage: &Coverage) -> bool {
    coverage.read_errors > 0 || coverage.parse_errors > 0 || coverage.analyzed == 0
}

fn compare_duplication(before: &Report, after: &Report) -> DuplicationComparison {
    let mut result = DuplicationComparison {
        reason: String::new(),
        before_status: before.duplication.status,
        after_status: after.duplication.status,
        before_clones: before.duplication.clones.clone(),
        after_clones: after.duplication.clones.clone(),
        before_config: before.config.duplication.clone(),
        after_config: after.config.duplication.clone(),
        comparable: true,
    };

    if result.before_status != DuplicationStatus::Measured
        || result.after_status != DuplicationStatus::Measured
    {
        result.comparable = false;
        result.reason = "duplication is comparable only when both reports measured it".to_owned();
    } else if !same_duplication_settings(before, after) {
        result.comparable = false;
        result.reason = "duplication configuration differs".to_owned();
    }

    result
}

/// `after - before` for every metric; the mass share delta exists only when
/// both buckets have a share.
fn metric_delta(before: &MetricBucket, after: &MetricBucket) -> MetricDelta {
    let share = match (
        before.high_complexity_mass_share,
        after.high_complexity_mass_share,
    ) {
        (Some(before), Some(after)) => Some(after - before),
        _ => None,
    };

    MetricDelta {
        high_complexity_mass_share: share,
        code_lines: i64::from(after.code_lines) - i64::from(before.code_lines),
        functions: i64::from(after.functions) - i64::from(before.functions),
        cyclomatic: i64::from(after.cyclomatic) - i64::from(before.cyclomatic),
        max_nesting: i64::from(after.max_nesting) - i64::from(before.max_nesting),
        mass: after.mass - before.mass,
        high_complexity_functions: i64::from(after.high_complexity_functions)
            - i64::from(before.high_complexity_functions),
        high_complexity_mass: after.high_complexity_mass - before.high_complexity_mass,
    }
}

/// One matched pair of inventory indexes.
struct Pair {
    basis: MatchBasis,
    before: usize,
    after: usize,
}

/// Several indexes that share a key and so cannot be paired one-to-one.
struct Group {
    basis: MatchBasis,
    before: Vec<usize>,
    after: Vec<usize>,
}

/// Result of matching two inventories. `before_used` and `after_used` mark
/// indexes that were paired or are part of an ambiguity.
struct Matches {
    pairs: Vec<Pair>,
    groups: Vec<Group>,
    before_used: Vec<bool>,
    after_used: Vec<bool>,
}

/// Pairs `before` with `after`: by ID, then by fingerprint among the
/// leftovers. A key with several entries on either side becomes a group, and
/// a pair involving an identity-ambiguous item is demoted to a group.
fn match_inventory<T: Tracked>(before: &[T], after: &[T]) -> Matches {
    let mut matches = Matches {
        pairs: Vec::new(),
        groups: Vec::new(),
        before_used: vec![false; before.len()],
        after_used: vec![false; after.len()],
    };

    let before_ids = index_by(before, T::id, &matches.before_used);
    let after_ids = index_by(after, T::id, &matches.after_used);
    match_keys(&mut matches, &before_ids, &after_ids, MatchBasis::Id);

    let before_fingerprints = index_by(before, T::fingerprint, &matches.before_used);
    let after_fingerprints = index_by(after, T::fingerprint, &matches.after_used);
    match_keys(
        &mut matches,
        &before_fingerprints,
        &after_fingerprints,
        MatchBasis::Fingerprint,
    );

    let (kept, demoted): (Vec<Pair>, Vec<Pair>) = std::mem::take(&mut matches.pairs)
        .into_iter()
        .partition(|pair| {
            !before[pair.before].identity_ambiguous() && !after[pair.after].identity_ambiguous()
        });

    matches.pairs = kept;
    matches.groups.extend(demoted.into_iter().map(|pair| Group {
        basis: pair.basis,
        before: vec![pair.before],
        after: vec![pair.after],
    }));

    matches
}

/// Indexes of unused items by non-empty key.
fn index_by<'a, T>(
    items: &'a [T],
    key: impl Fn(&'a T) -> &'a str,
    used: &[bool],
) -> BTreeMap<&'a str, Vec<usize>> {
    let mut indexes: BTreeMap<&str, Vec<usize>> = BTreeMap::new();

    for (index, item) in items.iter().enumerate() {
        let key = key(item);

        if !key.is_empty() && !used[index] {
            indexes.entry(key).or_default().push(index);
        }
    }

    indexes
}

fn match_keys(
    matches: &mut Matches,
    before: &BTreeMap<&str, Vec<usize>>,
    after: &BTreeMap<&str, Vec<usize>>,
    basis: MatchBasis,
) {
    let keys: BTreeSet<&str> = before.keys().chain(after.keys()).copied().collect();

    for key in keys {
        let before_indexes = before.get(key).map_or(&[][..], Vec::as_slice);
        let after_indexes = after.get(key).map_or(&[][..], Vec::as_slice);

        match (before_indexes, after_indexes) {
            ([before_index], [after_index]) => {
                matches.pairs.push(Pair {
                    basis,
                    before: *before_index,
                    after: *after_index,
                });
                matches.before_used[*before_index] = true;
                matches.after_used[*after_index] = true;
            }
            (before_indexes, after_indexes)
                if before_indexes.len() > 1 || after_indexes.len() > 1 =>
            {
                matches.groups.push(Group {
                    basis,
                    before: before_indexes.to_vec(),
                    after: after_indexes.to_vec(),
                });

                for &index in before_indexes {
                    matches.before_used[index] = true;
                }

                for &index in after_indexes {
                    matches.after_used[index] = true;
                }
            }
            _ => {}
        }
    }
}

/// The review items of `kind` by ID.
fn review_lookup(ledger: &ReviewLedger, kind: ReviewKind) -> HashMap<&str, &ReviewItem> {
    ledger
        .items
        .iter()
        .filter(|item| item.kind == kind && !item.id.is_empty())
        .map(|item| (item.id.as_str(), item))
        .collect()
}

/// Every change between two inventories, sorted, plus the ambiguous matches.
/// The change list holds matches, ambiguities, removals, then additions.
fn compare_inventory<T: Tracked>(
    before_items: &[T],
    after_items: &[T],
    before_review: &ReviewLedger,
    after_review: &ReviewLedger,
    after_paths: &SourcePaths<'_>,
) -> (Vec<Change<T>>, Vec<AmbiguousMatch>) {
    let mut before = before_items.to_vec();
    let mut after = after_items.to_vec();
    before.sort_by_cached_key(T::sort_key);
    after.sort_by_cached_key(T::sort_key);

    let matches = match_inventory(&before, &after);
    let before_reviews = review_lookup(before_review, T::KIND);
    let after_reviews = review_lookup(after_review, T::KIND);
    let reviewed_before = |item: &T| before_reviews.get(item.id()).map(|&review| review.clone());
    let reviewed_after = |item: &T| after_reviews.get(item.id()).map(|&review| review.clone());

    let mut changes = Vec::with_capacity(before.len() + after.len());

    for pair in &matches.pairs {
        let (before_item, after_item) = (&before[pair.before], &after[pair.after]);

        changes.push(Change {
            match_basis: Some(pair.basis),
            before: Some(before_item.clone()),
            after: Some(after_item.clone()),
            before_review: reviewed_before(before_item),
            after_review: reviewed_after(after_item),
            ..bare_change(T::matched_state(before_item, after_item))
        });
    }

    let mut ambiguous = Vec::with_capacity(matches.groups.len());

    for group in &matches.groups {
        let mut before_ids: Vec<String> = group
            .before
            .iter()
            .map(|&index| before[index].id().to_owned())
            .collect();
        let mut after_ids: Vec<String> = group
            .after
            .iter()
            .map(|&index| after[index].id().to_owned())
            .collect();
        before_ids.sort();
        after_ids.sort();

        let first_before = group.before.first().map(|&index| &before[index]);
        let first_after = group.after.first().map(|&index| &after[index]);

        changes.push(Change {
            match_basis: Some(group.basis),
            before: first_before.cloned(),
            after: first_after.cloned(),
            before_review: first_before.and_then(reviewed_before),
            after_review: first_after.and_then(reviewed_after),
            before_ids: before_ids.clone(),
            after_ids: after_ids.clone(),
            ..bare_change(ChangeState::Ambiguous)
        });

        ambiguous.extend(first_before.or(first_after).map(|subject| AmbiguousMatch {
            match_basis: Some(group.basis),
            before_ids,
            after_ids,
            ..subject.ambiguous_summary()
        }));
    }

    for (index, item) in before.iter().enumerate() {
        if !matches.before_used[index] {
            changes.push(Change {
                before: Some(item.clone()),
                before_review: reviewed_before(item),
                ..bare_change(item.removed_state(after_paths))
            });
        }
    }

    for (index, item) in after.iter().enumerate() {
        if !matches.after_used[index] {
            changes.push(Change {
                after: Some(item.clone()),
                after_review: reviewed_after(item),
                ..bare_change(ChangeState::New)
            });
        }
    }

    changes.sort_by_cached_key(|change| {
        change
            .after
            .as_ref()
            .or(change.before.as_ref())
            .map_or_else(String::new, |item| item.change_key(change.state))
    });
    ambiguous.sort_by_cached_key(|group| {
        [
            group.kind.as_str(),
            group.path.as_str(),
            group.rule.as_str(),
            group.message.as_str(),
        ]
        .join(":")
    });

    (changes, ambiguous)
}
