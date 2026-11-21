//! Linked locations and source blocks shared by both pages.

use crate::render::Source;
use crate::report::Location;

/// Longest source block shown whole; longer ones are clipped with a note.
const MAX_BLOCK_LINES: u32 = 400;

/// A `path:line` that links to the code when the source has a link base.
#[derive(Debug, Clone)]
pub(super) struct Place {
    pub(super) text: String,
    pub(super) href: Option<String>,
}

impl Place {
    pub(super) fn new(source: &Source, location: &Location) -> Self {
        Self {
            text: format!("{}:{}", location.path, location.start.line),
            href: source.link(&location.path, location.start.line, location.end.line),
        }
    }
}

/// Numbered source lines of one location.
#[derive(Debug, Clone)]
pub(super) struct Code {
    pub(super) caption: String,
    pub(super) lines: Vec<CodeLine>,
    /// Set when the block was clipped at `MAX_BLOCK_LINES`.
    pub(super) clipped: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct CodeLine {
    pub(super) number: u32,
    pub(super) text: String,
    pub(super) differs: bool,
}

impl Code {
    /// The lines of `location` read from the source root. `None` when there
    /// is no root, the file is unreadable, or it is shorter than the range.
    pub(super) fn read(source: &Source, location: &Location) -> Option<Self> {
        let start = location.start.line;
        let total = location.end.line.checked_sub(start)? + 1;
        let last = location.end.line.min(start + MAX_BLOCK_LINES - 1);
        let text = source.lines(&location.path, start, last)?;

        Some(Self {
            caption: Place::new(source, location).text,
            lines: text
                .into_iter()
                .zip(start..)
                .map(|(text, number)| CodeLine {
                    number,
                    text,
                    differs: false,
                })
                .collect(),
            clipped: (total > MAX_BLOCK_LINES)
                .then(|| format!("Showing the first {MAX_BLOCK_LINES} of {total} lines.")),
        })
    }

    /// Marks the lines that differ between two copies, compared line by line
    /// after trimming whitespace. A line with no counterpart differs.
    pub(super) fn mark_differences(left: &mut Self, right: &mut Self) {
        let total = left.lines.len().max(right.lines.len());

        for index in 0..total {
            let differs = match (left.lines.get(index), right.lines.get(index)) {
                (Some(one), Some(other)) => one.text.trim() != other.text.trim(),
                _ => true,
            };

            for side in [&mut *left, &mut *right] {
                if let Some(line) = side.lines.get_mut(index) {
                    line.differs = differs;
                }
            }
        }
    }
}
