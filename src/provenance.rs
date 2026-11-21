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
