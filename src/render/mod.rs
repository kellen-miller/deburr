//! Report and comparison output in text, markdown, HTML, GitHub, and JSON.
//!
//! JSON is the complete, stable contract. The four human formats (text,
//! markdown, HTML, GitHub) render the one shared [`summary`] (verdict, hints,
//! ranked worklist, concentration) at different depths and carry no stability
//! promise. [`Source`] says where code lives (links, snippets) and [`Style`]
//! whether color is allowed; both depend on where rendering happens, not on
//! the analysis.

use std::io::{self, Write};

use clap::ValueEnum;
use serde::Serialize;

use crate::report::{Comparison, Report};

#[doc(hidden)]
pub mod filters;
pub mod github;
pub mod html;
pub mod markdown;
pub mod source;
pub mod summary;
pub mod text;

pub use source::Source;

/// Output formats accepted by `--format`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Format {
    Text,
    Json,
    Markdown,
    Html,
    Github,
}

impl Format {
    /// The name accepted by `--format`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Json => "json",
            Self::Markdown => "markdown",
            Self::Html => "html",
            Self::Github => "github",
        }
    }
}

/// Whether ANSI color may be written. Only the text renderer reads it.
#[derive(Debug, Clone, Copy, Default)]
pub struct Style {
    pub color: bool,
}

/// Writes `value` as pretty JSON followed by a newline.
///
/// # Errors
///
/// Returns an I/O error when encoding or writing fails.
pub fn write_json<T: Serialize>(value: &T, out: &mut dyn Write) -> io::Result<()> {
    serde_json::to_writer_pretty(&mut *out, value)?;
    out.write_all(b"\n")
}

/// Writes `report` in `format`.
///
/// # Errors
///
/// Returns an I/O error when writing fails.
pub fn render_report(
    report: &Report,
    source: &Source,
    style: Style,
    format: Format,
    out: &mut dyn Write,
) -> io::Result<()> {
    match format {
        Format::Text => text::report(report, source, style, out),
        Format::Markdown => markdown::report(report, source, style, out),
        Format::Json => write_json(report, out),
        Format::Html => html::report(report, source, style, out),
        Format::Github => github::report(report, source, style, out),
    }
}

/// Writes `comparison` in `format`.
///
/// # Errors
///
/// Returns an I/O error when writing fails.
pub fn render_comparison(
    comparison: &Comparison,
    source: &Source,
    style: Style,
    format: Format,
    out: &mut dyn Write,
) -> io::Result<()> {
    match format {
        Format::Text => text::comparison(comparison, source, style, out),
        Format::Markdown => markdown::comparison(comparison, source, style, out),
        Format::Json => write_json(comparison, out),
        Format::Html => html::comparison(comparison, source, style, out),
        Format::Github => github::comparison(comparison, source, style, out),
    }
}

/// The serialized (snake_case) name of a plain enum such as a file status,
/// so renderers print exactly what the JSON report contains.
pub(crate) fn enum_name<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(name)) => name,
        _ => String::new(),
    }
}
