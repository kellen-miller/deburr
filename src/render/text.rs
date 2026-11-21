//! Plain-text rendering and the embedded cleanup guide.

use std::io::{self, Write};

use super::not_implemented;
use crate::report::{Comparison, Report};

/// Writes the text report.
///
/// # Errors
///
/// Returns an I/O error when writing fails.
pub fn report(_report: &Report, _out: &mut dyn Write) -> io::Result<()> {
    Err(not_implemented("text report rendering"))
}

/// Writes the text comparison.
///
/// # Errors
///
/// Returns an I/O error when writing fails.
pub fn comparison(_comparison: &Comparison, _out: &mut dyn Write) -> io::Result<()> {
    Err(not_implemented("text comparison rendering"))
}

/// Writes the cleanup guide printed by `deburr guide cleanup`.
///
/// # Errors
///
/// Returns an I/O error when writing fails.
pub fn cleanup_guide(_out: &mut dyn Write) -> io::Result<()> {
    Err(not_implemented("the cleanup guide"))
}
