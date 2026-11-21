//! Askama filters shared by the human-facing templates.

use std::fmt::Display;

use askama::{Result, Values, filter_fn};

use super::summary::count as grouped;

/// A number with thousands separators: `8219` as `8,219`.
#[filter_fn]
pub fn count(value: &u32, _: &dyn Values) -> Result<String> {
    Ok(grouped(*value))
}

/// A complexity weight as a whole number with separators.
#[filter_fn]
pub fn weight(mass: &f64, _: &dyn Values) -> Result<String> {
    Ok(format!("{:.0}", mass.max(0.0))
        .parse()
        .map_or_else(|_| "0".to_owned(), grouped))
}

/// `text` escaped for markdown by [`escape`].
#[filter_fn]
pub fn md(text: impl Display, _: &dyn Values) -> Result<String> {
    Ok(escape(&text.to_string()))
}

/// Escapes text taken from a report so it cannot form markdown, HTML, or a
/// table cell boundary. Line breaks become spaces.
#[must_use]
pub fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());

    for character in text.chars() {
        match character {
            '|' | '<' | '>' | '`' | '*' | '\\' | '[' | ']' => {
                escaped.push('\\');
                escaped.push(character);
            }
            '\n' | '\r' => escaped.push(' '),
            other => escaped.push(other),
        }
    }

    escaped
}
