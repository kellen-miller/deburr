//! Where the audited code lives at render time: permalinks and snippets.
//!
//! Neither is stored in a report. Both depend on where rendering happens (a
//! CI runner, a laptop), not on the analysis, so the CLI builds one `Source`
//! per run and hands it to every renderer.

use std::fs;
use std::path::{Component, Path, PathBuf};

/// Rendering-time knowledge about the audited source.
#[derive(Debug, Clone, Default)]
pub struct Source {
    root: Option<PathBuf>,
    link_base: Option<String>,
    path_prefix: String,
    name: String,
}

impl Source {
    /// `root` is the directory report paths are relative to, `link_base` the
    /// permalink base (`https://github.com/o/r/blob/<commit>`), and
    /// `path_prefix` the repository-relative directory of `root`, with a
    /// trailing slash (empty at the repository root).
    #[must_use]
    pub const fn new(
        root: Option<PathBuf>,
        link_base: Option<String>,
        path_prefix: String,
    ) -> Self {
        Self {
            root,
            link_base,
            path_prefix,
            name: String::new(),
        }
    }

    /// Names the audited tree for report headings.
    #[must_use]
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();

        self
    }

    /// The audited directory's name, or `report` when unknown. This is the
    /// `name` renderers pass to `summarize`.
    #[must_use]
    pub fn name(&self) -> &str {
        if self.name.is_empty() {
            "report"
        } else {
            &self.name
        }
    }

    /// `<link_base>/<path_prefix><path>#L<start>-L<end>`, or `#L<start>` when
    /// the range is one line. `None` without a link base.
    #[must_use]
    pub fn link(&self, path: &str, start: u32, end: u32) -> Option<String> {
        let base = self.link_base.as_deref()?;
        let target = format!("{}{path}", self.path_prefix);
        let mut link = String::with_capacity(base.len() + target.len() + 16);

        link.push_str(base);
        link.push('/');

        for byte in target.bytes() {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/') {
                link.push(char::from(byte));
            } else {
                link.push_str(&format!("%{byte:02X}"));
            }
        }

        if end > start {
            link.push_str(&format!("#L{start}-L{end}"));
        } else {
            link.push_str(&format!("#L{start}"));
        }

        Some(link)
    }

    /// Lines `start..=end` (1-based) of `root/path`. `None` without a root,
    /// when the file cannot be read, when `path` leaves the root (including through symlinks), or when the
    /// file is shorter than `end`.
    #[must_use]
    pub fn lines(&self, path: &str, start: u32, end: u32) -> Option<Vec<String>> {
        let root = self.root.as_deref()?;
        let relative = Path::new(path);

        if start == 0
            || end < start
            || !relative
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
        {
            return None;
        }

        // Canonicalize both sides so a symlinked file or directory cannot
        // lead outside the root.
        let root = fs::canonicalize(root).ok()?;
        let candidate = fs::canonicalize(root.join(relative)).ok()?;

        if !candidate.starts_with(&root) {
            return None;
        }

        let text = fs::read_to_string(candidate).ok()?;
        let skipped = usize::try_from(start - 1).ok()?;
        let wanted = usize::try_from(end - start + 1).ok()?;
        let lines: Vec<String> = text
            .lines()
            .skip(skipped)
            .take(wanted)
            .map(str::to_owned)
            .collect();

        (lines.len() == wanted).then_some(lines)
    }
}
