//! HTML pages for a report and a comparison.
//!
//! Both render the shared summary into one self-contained file: inline CSS
//! and JavaScript, system fonts, and no external loads, so a saved report
//! works offline. The full JSON is embedded for tools. Askama escapes every
//! interpolated value; only the static CSS and JS and the escaped JSON are
//! written unescaped.

mod code;
mod comparison;
mod report;

pub use comparison::comparison;
pub use report::report;

use std::io;

use serde::Serialize;

use super::summary::count;
use crate::report::{DuplicationConfig, SourceProvenance};

/// The stylesheet both pages share.
const CSS: &str = include_str!("../../../templates/page.css");

/// Most worklist items (or comparison entries) that carry source. A very
/// large repository would otherwise produce a page too big to open.
const SOURCE_ITEMS: usize = 300;

/// Whether the audited tree matched its commit.
fn working_tree(source: &SourceProvenance) -> &'static str {
    match (source.git_known, source.dirty) {
        (false, _) => "unknown",
        (true, true) => "uncommitted changes",
        (true, false) => "clean",
    }
}

/// The clone size limits in words, or that duplication was not measured.
fn clone_minimum(config: &DuplicationConfig) -> String {
    if config.requested {
        format!(
            "{} tokens and {} code lines",
            count(config.min_tokens),
            count(config.min_lines)
        )
    } else {
        "duplication not measured".to_owned()
    }
}

/// Serializes `value` for a `<script type="application/json">` element.
/// Every `<` becomes `<`, so text such as `</script>` inside a function
/// name or message cannot end the element.
fn embedded_json<T: Serialize>(value: &T) -> io::Result<String> {
    Ok(serde_json::to_string(value)?.replace('<', "\\u003c"))
}
