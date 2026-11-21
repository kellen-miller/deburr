//! GitHub workflow-command rendering.

use std::io::{self, Write};

use super::not_implemented;
use crate::report::{Comparison, Report};

/// Writes the report as `::notice`/`::warning`/`::error` commands.
///
/// # Errors
///
/// Returns an I/O error when writing fails.
pub fn report(_report: &Report, _out: &mut dyn Write) -> io::Result<()> {
    Err(not_implemented("GitHub report rendering"))
}

/// Writes the comparison as workflow commands.
///
/// # Errors
///
/// Returns an I/O error when writing fails.
pub fn comparison(_comparison: &Comparison, _out: &mut dyn Write) -> io::Result<()> {
    Err(not_implemented("GitHub comparison rendering"))
}
