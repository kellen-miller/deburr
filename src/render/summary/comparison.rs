//! Comparison summary: what got worse, what improved, what could not be
//! matched.

use super::groups::{Group, group_clones};
use super::{ChangeEntry, ComparisonSummary, WorkKind, count, cyclomatic_words, location_of};
use crate::report::{ChangeState, CodeClone, Comparison, Finding, Function, Location};

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
