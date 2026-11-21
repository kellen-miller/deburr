//! Report and comparison output in text, JSON, HTML, and GitHub formats.

use std::io::{self, Write};

use clap::ValueEnum;

use crate::report::{Comparison, Report};

pub mod github;
pub mod html;
pub mod text;

/// Output formats accepted by `--format`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Format {
    Text,
    Json,
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
            Self::Html => "html",
            Self::Github => "github",
        }
    }
}

fn not_implemented(what: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        format!("{what} is not implemented yet"),
    )
}

/// Writes `value` as pretty JSON followed by a newline.
///
/// # Errors
///
/// Returns an I/O error when encoding or writing fails.
pub fn write_json<T: serde::Serialize>(value: &T, out: &mut dyn Write) -> io::Result<()> {
    serde_json::to_writer_pretty(&mut *out, value)?;
    out.write_all(b"\n")
}

/// Writes `report` in `format`.
///
/// # Errors
///
/// Returns an I/O error when writing fails or the format is not implemented yet.
pub fn render_report(report: &Report, format: Format, out: &mut dyn Write) -> io::Result<()> {
    match format {
        Format::Text => text::report(report, out),
        Format::Json => write_json(report, out),
        Format::Html => html::report(report, out),
        Format::Github => github::report(report, out),
    }
}

/// Writes `comparison` in `format`.
///
/// # Errors
///
/// Returns an I/O error when writing fails or the format is not implemented yet.
pub fn render_comparison(
    comparison: &Comparison,
    format: Format,
    out: &mut dyn Write,
) -> io::Result<()> {
    match format {
        Format::Text => text::comparison(comparison, out),
        Format::Json => write_json(comparison, out),
        Format::Html => html::comparison(comparison, out),
        Format::Github => github::comparison(comparison, out),
    }
}
