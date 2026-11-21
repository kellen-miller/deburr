//! Rewrites report and comparison paths to be workspace-relative, so GitHub
//! annotations land on the right files when the audited directory is a
//! subdirectory of the workspace.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::report::{Change, Comparison, Report, ReviewItem};

/// The audited directory's path relative to `GITHUB_WORKSPACE` (or the
/// current directory), when it is inside it.
pub(super) fn github_path_prefix(target: &Path) -> Option<String> {
    let workspace = env::var_os("GITHUB_WORKSPACE")
        .map(PathBuf::from)
        .or_else(|| env::current_dir().ok())?;
    let workspace = fs::canonicalize(workspace).ok()?;
    let target = fs::canonicalize(target)
        .ok()
        .filter(|target| target.is_dir())?;
    let relative = target.strip_prefix(&workspace).ok()?;
    let prefix = relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");

    (!prefix.is_empty()).then_some(prefix)
}

/// Rewrites every source path in `report` to be workspace-relative.
pub(super) fn prefix_paths(report: &mut Report, prefix: &str) {
    let prefixed = |path: &mut String| *path = format!("{prefix}/{path}");

    report
        .files
        .iter_mut()
        .for_each(|file| prefixed(&mut file.path));
    report
        .functions
        .iter_mut()
        .for_each(|function| prefixed(&mut function.path));
    report
        .findings
        .iter_mut()
        .for_each(|finding| prefixed(&mut finding.path));

    for clone in &mut report.duplication.clones {
        clone
            .endpoints
            .iter_mut()
            .for_each(|endpoint| prefixed(&mut endpoint.path));
    }

    if let Some(ledger) = &mut report.review {
        for item in &mut ledger.items {
            item.locations
                .iter_mut()
                .for_each(|location| prefixed(&mut location.path));
        }
    }
}

/// Rewrites every source path in `comparison` to be workspace-relative.
pub(super) fn prefix_comparison_paths(comparison: &mut Comparison, prefix: &str) {
    let prefixed = |path: &mut String| *path = format!("{prefix}/{path}");

    for change in &mut comparison.function_changes {
        for function in [&mut change.before, &mut change.after]
            .into_iter()
            .flatten()
        {
            prefixed(&mut function.path);
        }

        prefix_change_reviews(change, prefix);
    }

    for change in &mut comparison.finding_changes {
        for finding in [&mut change.before, &mut change.after]
            .into_iter()
            .flatten()
        {
            prefixed(&mut finding.path);
        }

        prefix_change_reviews(change, prefix);
    }

    for change in &mut comparison.clone_changes {
        for clone in [&mut change.before, &mut change.after]
            .into_iter()
            .flatten()
        {
            clone
                .endpoints
                .iter_mut()
                .for_each(|endpoint| prefixed(&mut endpoint.path));
        }

        prefix_change_reviews(change, prefix);
    }

    for finding in comparison
        .new_findings
        .iter_mut()
        .chain(&mut comparison.resolved_findings)
    {
        prefixed(&mut finding.path);
    }

    let clones = comparison
        .duplication
        .before_clones
        .iter_mut()
        .chain(&mut comparison.duplication.after_clones);

    for clone in clones {
        clone
            .endpoints
            .iter_mut()
            .for_each(|endpoint| prefixed(&mut endpoint.path));
    }

    let ambiguous = comparison
        .ambiguous_functions
        .iter_mut()
        .chain(&mut comparison.ambiguous_findings)
        .chain(&mut comparison.ambiguous_clones);

    for entry in ambiguous {
        prefixed(&mut entry.path);
    }

    for ledger in [&mut comparison.before_review, &mut comparison.after_review] {
        for item in &mut ledger.items {
            prefix_item(item, prefix);
        }
    }
}

fn prefix_change_reviews<T>(change: &mut Change<T>, prefix: &str) {
    for item in [&mut change.before_review, &mut change.after_review]
        .into_iter()
        .flatten()
    {
        prefix_item(item, prefix);
    }
}

fn prefix_item(item: &mut ReviewItem, prefix: &str) {
    item.locations
        .iter_mut()
        .for_each(|location| location.path = format!("{prefix}/{}", location.path));
}
