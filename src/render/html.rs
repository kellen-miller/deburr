//! HTML rendering (askama templates arrive with the renderer milestone).

use std::io::{self, Write};

use super::not_implemented;
use crate::report::{Comparison, Report};

/// Writes the HTML report.
///
/// # Errors
///
/// Returns an I/O error when writing fails.
pub fn report(_report: &Report, _out: &mut dyn Write) -> io::Result<()> {
    Err(not_implemented("HTML report rendering"))
}

/// Writes the HTML comparison.
///
/// # Errors
///
/// Returns an I/O error when writing fails.
pub fn comparison(_comparison: &Comparison, _out: &mut dyn Write) -> io::Result<()> {
    Err(not_implemented("HTML comparison rendering"))
}
