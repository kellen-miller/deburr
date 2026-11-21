//! Where the audited source and this deburr build came from.

use std::path::Path;
use std::process::{Command, Stdio};

use crate::report::{BuildProvenance, SourceProvenance};

#[derive(Debug, thiserror::Error)]
pub enum ProvenanceError {
    #[error("run git: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("git {command} failed")]
    Git { command: &'static str },
}

/// Records the audited tree's git commit and dirty state by running `git`
/// in `dir`. `manifest_digest` is carried into the result unchanged.
///
/// # Errors
///
/// Returns an error when `git` cannot be run or cannot report the commit or
/// status (for example, outside a repository).
pub fn source(dir: &Path, manifest_digest: &str) -> Result<SourceProvenance, ProvenanceError> {
    let commit = git_output(dir, &["rev-parse", "HEAD"], "rev-parse")?;
    let status = git_output(
        dir,
        &["status", "--porcelain=v1", "--untracked-files=all"],
        "status",
    )?;

    let commit = commit.trim().to_owned();
    let git_known = !commit.is_empty();

    Ok(SourceProvenance {
        dirty: git_known && !status.trim().is_empty(),
        commit,
        manifest_digest: manifest_digest.to_owned(),
        git_known,
    })
}

/// This deburr build's own identity.
#[must_use]
pub fn build() -> BuildProvenance {
    BuildProvenance {
        version: crate::VERSION.to_owned(),
    }
}

/// `https://github.com/<owner>/<repo>/blob/<commit>` when `dir`'s origin
/// remote is on GitHub (`git@github.com:`, `https://github.com/` or
/// `ssh://git@github.com/` forms); `None` otherwise.
#[must_use]
pub fn github_blob_base(dir: &Path, commit: &str) -> Option<String> {
    if commit.is_empty() {
        return None;
    }

    let remote = git_output(dir, &["remote", "get-url", "origin"], "remote-url").ok()?;
    let repository = github_repository(remote.trim())?;

    Some(format!("https://github.com/{repository}/blob/{commit}"))
}

/// `dir`'s path relative to its repository root, with a trailing slash
/// (empty at the root); `None` outside a repository.
#[must_use]
pub fn repository_prefix(dir: &Path) -> Option<String> {
    let prefix = git_output(dir, &["rev-parse", "--show-prefix"], "show-prefix").ok()?;

    Some(prefix.trim().to_owned())
}

/// `owner/repo` from a GitHub remote URL.
fn github_repository(remote: &str) -> Option<String> {
    let path = [
        "git@github.com:",
        "https://github.com/",
        "ssh://git@github.com/",
    ]
    .iter()
    .find_map(|prefix| remote.strip_prefix(prefix))?;
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let (owner, repo) = path.split_once('/')?;
    let valid = |part: &str| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };

    (valid(owner) && valid(repo)).then(|| format!("{owner}/{repo}"))
}

fn git_output(dir: &Path, args: &[&str], command: &'static str) -> Result<String, ProvenanceError> {
    let output = Command::new("git")
        .args(["-c", "core.fsmonitor=false", "-C"])
        .arg(dir)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()?;

    if !output.status.success() {
        return Err(ProvenanceError::Git { command });
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::github_repository;

    #[test]
    fn github_remotes_in_every_supported_form() {
        for remote in [
            "git@github.com:owner/repo.git",
            "git@github.com:owner/repo",
            "https://github.com/owner/repo.git",
            "https://github.com/owner/repo/",
            "ssh://git@github.com/owner/repo.git",
        ] {
            assert_eq!(
                github_repository(remote).as_deref(),
                Some("owner/repo"),
                "{remote}"
            );
        }
    }

    #[test]
    fn other_hosts_and_malformed_remotes_are_rejected() {
        for remote in [
            "git@gitlab.com:owner/repo.git",
            "https://example.com/owner/repo",
            "https://github.com/owner",
            "https://github.com/owner/repo/extra",
            "/local/path/repo",
            "",
        ] {
            assert_eq!(github_repository(remote), None, "{remote}");
        }
    }
}
