//! Plain-text rendering and the embedded cleanup guide.

use std::io::{self, Write};

use super::{
    clone_text, endpoint_text, enum_name, finding_text, function_text, ledger_of, location,
    provenance_text, ranked_functions, review_fields,
};
use crate::report::{
    AmbiguousMatch, Category, Change, ChangeState, Comparison, FileStatus, Function, MetricBucket,
    MetricDelta, Report, ReviewCounts, ReviewItem,
};

/// Functions listed per hotspot bucket; JSON and HTML carry them all.
const HOTSPOT_LIMIT: usize = 20;

const CLEANUP_GUIDE: &str = include_str!("../../docs/cleanup.md");

/// Writes the text report.
///
/// # Errors
///
/// Returns an I/O error when writing fails.
pub fn report(report: &Report, out: &mut dyn Write) -> io::Result<()> {
    writeln!(out, "Deburr report (schema {})", report.schema_version)?;
    writeln!(
        out,
        "Analyzer: {} {}",
        report.analyzer.id, report.analyzer.version
    )?;
    writeln!(out, "Provenance: {}", provenance_text(&report.provenance))?;
    writeln!(out, "Language: {}", report.config.language)?;
    writeln!(out, "Scope: {}", report.scope.languages.join(","))?;

    let coverage = &report.coverage;
    writeln!(
        out,
        "Coverage: discovered={} analyzed={} excluded={} unsupported={} read_errors={} parse_errors={} symlinks={} excluded_directories={}",
        coverage.discovered,
        coverage.analyzed,
        coverage.excluded,
        coverage.unsupported,
        coverage.read_errors,
        coverage.parse_errors,
        coverage.symlinks,
        coverage.excluded_directories,
    )?;
    if coverage.analyzed == 0 {
        writeln!(out, "Analysis: no analyzable Go or TypeScript files")?;
    }

    metric_bucket(out, "All", &report.metrics.all)?;
    metric_bucket(out, "Production", &report.metrics.production)?;
    metric_bucket(out, "Test", &report.metrics.test)?;
    writeln!(
        out,
        "Files: {} discovered entries; use JSON/HTML for the full inventory",
        report.files.len()
    )?;

    let failures: Vec<_> = report
        .files
        .iter()
        .filter(|file| matches!(file.status, FileStatus::ReadError | FileStatus::ParseError))
        .collect();
    if !failures.is_empty() {
        writeln!(out, "Source failures ({})", failures.len())?;
        for file in failures {
            let message = if file.error.is_empty() {
                &file.reason
            } else {
                &file.error
            };

            writeln!(
                out,
                "- {} [{}] {message}",
                file.path,
                enum_name(&file.status)
            )?;
        }
    }

    writeln!(
        out,
        "\nFunction hotspots (ranked by raw mass; JSON/HTML include all {})",
        report.functions.len()
    )?;
    let in_bucket = |keep: fn(Category) -> bool| -> Vec<&Function> {
        report
            .functions
            .iter()
            .filter(|function| keep(function.category))
            .collect()
    };
    let other = in_bucket(|category| !matches!(category, Category::Production | Category::Test));
    hotspots(
        out,
        "Production",
        &in_bucket(|category| category == Category::Production),
    )?;
    hotspots(
        out,
        "Test",
        &in_bucket(|category| category == Category::Test),
    )?;
    if !other.is_empty() {
        hotspots(out, "Other", &other)?;
    }

    writeln!(out, "\nFindings ({})", report.findings.len())?;
    for finding in &report.findings {
        write!(
            out,
            "- {}:{}:{}-{}:{} [{}] {}: {} identity_ambiguous={}",
            finding.path,
            finding.start.line,
            finding.start.column,
            finding.end.line,
            finding.end.column,
            finding.severity,
            finding.rule,
            finding.message,
            finding.identity_ambiguous,
        )?;
        if !finding.details.is_empty() {
            let details: Vec<String> = finding
                .details
                .iter()
                .map(|detail| format!("{}={}", detail.key, detail.value))
                .collect();
            write!(out, " ({})", details.join(", "))?;
        }

        writeln!(out)?;
    }

    let duplication = &report.duplication;
    write!(
        out,
        "\nDuplication: {} min_tokens={} min_lines={}",
        enum_name(&duplication.status),
        duplication.min_tokens,
        duplication.min_lines,
    )?;
    if !duplication.error.is_empty() {
        write!(out, " error={}", duplication.error)?;
    }

    writeln!(out)?;
    for measured in &duplication.categories {
        writeln!(
            out,
            "- {}: {:.2}% duplication ({} of {} code lines across {} files)",
            measured.category.as_str(),
            measured.percentage,
            measured.duplicated_code_lines,
            measured.code_lines,
            measured.files,
        )?;
    }

    for clone in &duplication.clones {
        writeln!(
            out,
            "- clone {} family={} category={} tokens={} ambiguous={}",
            clone.id,
            clone.family_id,
            clone.category.as_str(),
            clone.tokens,
            clone.identity_ambiguous,
        )?;
        for endpoint in &clone.endpoints {
            writeln!(out, "  - {}", endpoint_text(endpoint))?;
        }
    }

    let ledger = ledger_of(report);
    writeln!(out, "\nReview: {}", counts_text(&ledger.counts))?;
    for item in &ledger.items {
        review_item(out, item)?;
    }

    Ok(())
}

/// Writes the text comparison.
///
/// # Errors
///
/// Returns an I/O error when writing fails.
pub fn comparison(comparison: &Comparison, out: &mut dyn Write) -> io::Result<()> {
    writeln!(
        out,
        "Deburr comparison (schema {})",
        comparison.schema_version
    )?;
    writeln!(
        out,
        "Analyzer: {} {} -> {} {}",
        comparison.before_analyzer.id,
        comparison.before_analyzer.version,
        comparison.after_analyzer.id,
        comparison.after_analyzer.version,
    )?;
    writeln!(
        out,
        "Before provenance: {}",
        provenance_text(&comparison.before_provenance)
    )?;
    writeln!(
        out,
        "After provenance: {}",
        provenance_text(&comparison.after_provenance)
    )?;
    writeln!(out, "Compatible: {}", comparison.compatible)?;
    writeln!(out, "Complete: {}", comparison.complete)?;
    if !comparison.compatible || !comparison.complete {
        writeln!(
            out,
            "Change conclusion: unavailable; no finding, function, or clone improvement claim was made."
        )?;
    }

    if !comparison.reason.is_empty() {
        writeln!(out, "Reason: {}", comparison.reason)?;
    }

    let before = &comparison.before_coverage;
    let after = &comparison.after_coverage;
    writeln!(
        out,
        "Coverage: before analyzed={} read_errors={} parse_errors={}; after analyzed={} read_errors={} parse_errors={}",
        before.analyzed,
        before.read_errors,
        before.parse_errors,
        after.analyzed,
        after.read_errors,
        after.parse_errors,
    )?;
    if let Some(delta) = &comparison.metrics_delta {
        metric_delta(out, "All", &delta.all)?;
        metric_delta(out, "Production", &delta.production)?;
        metric_delta(out, "Test", &delta.test)?;
    } else {
        writeln!(out, "Metrics delta: unavailable")?;
    }

    writeln!(
        out,
        "Review before: {}",
        counts_text(&comparison.before_review.counts)
    )?;
    writeln!(
        out,
        "Review after: {}",
        counts_text(&comparison.after_review.counts)
    )?;

    writeln!(out, "New findings ({})", comparison.new_findings.len())?;
    for finding in &comparison.new_findings {
        writeln!(out, "- {}", finding_text(finding))?;
    }

    writeln!(
        out,
        "No longer reported findings ({})",
        comparison.resolved_findings.len()
    )?;
    for finding in &comparison.resolved_findings {
        writeln!(out, "- {}", finding_text(finding))?;
    }

    writeln!(
        out,
        "Finding changes ({})",
        comparison.finding_changes.len()
    )?;
    for change in &comparison.finding_changes {
        change_line(out, change, finding_text)?;
    }

    ambiguous_matches(out, "finding", &comparison.ambiguous_findings)?;

    writeln!(
        out,
        "Function changes ({})",
        comparison.function_changes.len()
    )?;
    for change in &comparison.function_changes {
        change_line(out, change, function_text)?;
        if matches!(
            change.state,
            ChangeState::ResolvedThreshold
                | ChangeState::DeletedFunction
                | ChangeState::SourceRemoved
        ) {
            writeln!(out, "  note=no_behavior_conclusion")?;
        }
    }

    ambiguous_matches(out, "function", &comparison.ambiguous_functions)?;

    writeln!(out, "Clone changes ({})", comparison.clone_changes.len())?;
    for change in &comparison.clone_changes {
        change_line(out, change, clone_text)?;
    }

    ambiguous_matches(out, "clone", &comparison.ambiguous_clones)?;

    let duplication = &comparison.duplication;
    write!(
        out,
        "Duplication: before={} after={} comparable={} before_config={}/{}/{} after_config={}/{}/{}",
        enum_name(&duplication.before_status),
        enum_name(&duplication.after_status),
        duplication.comparable,
        duplication.before_config.requested,
        duplication.before_config.min_tokens,
        duplication.before_config.min_lines,
        duplication.after_config.requested,
        duplication.after_config.min_tokens,
        duplication.after_config.min_lines,
    )?;
    if !duplication.reason.is_empty() {
        write!(out, " reason={}", duplication.reason)?;
    }

    writeln!(out)?;
    for (side, clones) in [
        ("before", &duplication.before_clones),
        ("after", &duplication.after_clones),
    ] {
        for clone in clones {
            writeln!(out, "- {side} clone {}", clone_text(clone))?;
        }
    }

    Ok(())
}

/// Writes the cleanup guide printed by `deburr guide cleanup`.
///
/// # Errors
///
/// Returns an I/O error when writing fails.
pub fn cleanup_guide(out: &mut dyn Write) -> io::Result<()> {
    out.write_all(CLEANUP_GUIDE.as_bytes())
}

fn counts_text(counts: &ReviewCounts) -> String {
    format!(
        "total={} unreviewed={} refactored={} retained={} deferred={} stale={}",
        counts.total,
        counts.unreviewed,
        counts.refactored,
        counts.retained,
        counts.deferred,
        counts.stale,
    )
}

fn metric_bucket(out: &mut dyn Write, name: &str, bucket: &MetricBucket) -> io::Result<()> {
    write!(
        out,
        "{name} metrics: code_lines={} functions={} cyclomatic={} max_nesting={} mass={:.6} high_complexity_functions={} high_complexity_mass={:.6}",
        bucket.code_lines,
        bucket.functions,
        bucket.cyclomatic,
        bucket.max_nesting,
        bucket.mass,
        bucket.high_complexity_functions,
        bucket.high_complexity_mass,
    )?;
    match bucket.high_complexity_mass_share {
        Some(share) => writeln!(out, " high_complexity_mass_share={share:.6}"),
        None => writeln!(out, " high_complexity_mass_share=null"),
    }
}

fn metric_delta(out: &mut dyn Write, name: &str, delta: &MetricDelta) -> io::Result<()> {
    write!(
        out,
        "{name} metric delta: code_lines={} functions={} cyclomatic={} max_nesting={} mass={:+.6} high_complexity_functions={} high_complexity_mass={:+.6}",
        delta.code_lines,
        delta.functions,
        delta.cyclomatic,
        delta.max_nesting,
        delta.mass,
        delta.high_complexity_functions,
        delta.high_complexity_mass,
    )?;
    match delta.high_complexity_mass_share {
        Some(share) => writeln!(out, " high_complexity_mass_share={share:+.6}"),
        None => writeln!(out, " high_complexity_mass_share=unavailable"),
    }
}

fn hotspots(out: &mut dyn Write, bucket: &str, functions: &[&Function]) -> io::Result<()> {
    let ranked = ranked_functions(functions.iter().copied());
    let shown = ranked.len().min(HOTSPOT_LIMIT);
    writeln!(out, "{bucket} hotspots ({shown} of {})", ranked.len())?;

    for function in &ranked[..shown] {
        let signals = &function.structural_signals;
        write!(
            out,
            "- {} {} [{}] sloc={} cyclomatic={} nesting={} mass={:.6}",
            location(&function.path, function.start, function.end),
            function.name,
            function.category.as_str(),
            function.sloc,
            function.cyclomatic,
            function.max_nesting,
            function.mass,
        )?;
        if function.high_complexity {
            write!(out, " high_complexity=true")?;
        }

        writeln!(
            out,
            " flat_guards={} field_mappings={} nested_branches={} assertion_like_calls={} identity_ambiguous={}",
            signals.flat_guards,
            signals.field_mappings,
            signals.nested_branches,
            signals.assertion_like_calls,
            function.identity_ambiguous,
        )?;
    }

    if ranked.len() > shown {
        writeln!(
            out,
            "  ... {} more; use JSON/HTML for the full inventory",
            ranked.len() - shown
        )?;
    }

    Ok(())
}

fn review_item(out: &mut dyn Write, item: &ReviewItem) -> io::Result<()> {
    write!(
        out,
        "- {} {} [{}]",
        enum_name(&item.kind),
        item.id,
        enum_name(&item.status)
    )?;
    if !item.locations.is_empty() {
        let locations: Vec<String> = item
            .locations
            .iter()
            .map(|at| location(&at.path, at.start, at.end))
            .collect();
        write!(out, " locations={}", locations.join(";"))?;
    }

    writeln!(out, "{}", review_fields(item))
}

/// One changed function, finding, or clone as a single line.
fn change_line<T>(
    out: &mut dyn Write,
    change: &Change<T>,
    describe: fn(&T) -> String,
) -> io::Result<()> {
    write!(out, "- {}", enum_name(&change.state))?;
    if let Some(basis) = &change.match_basis {
        write!(out, " match={}", enum_name(basis))?;
    }

    if let Some(before) = &change.before {
        write!(out, " before={}", describe(before))?;
    }

    if let Some(after) = &change.after {
        write!(out, " after={}", describe(after))?;
    }

    if !change.before_ids.is_empty() || !change.after_ids.is_empty() {
        write!(
            out,
            " before_ids={} after_ids={}",
            change.before_ids.join(","),
            change.after_ids.join(",")
        )?;
    }

    for (side, review) in [
        ("before", &change.before_review),
        ("after", &change.after_review),
    ] {
        if let Some(item) = review {
            write!(
                out,
                " {side}_review={}{}",
                enum_name(&item.status),
                review_fields(item)
            )?;
        }
    }

    writeln!(out)
}

fn ambiguous_matches(
    out: &mut dyn Write,
    kind: &str,
    matches: &[AmbiguousMatch],
) -> io::Result<()> {
    if matches.is_empty() {
        return Ok(());
    }

    writeln!(out, "Ambiguous {kind} matches ({})", matches.len())?;
    for ambiguous in matches {
        writeln!(
            out,
            "- {}:{}: {} before={} after={}",
            ambiguous.path,
            ambiguous.rule,
            ambiguous.message,
            ambiguous.before_ids.join(","),
            ambiguous.after_ids.join(","),
        )?;
    }

    Ok(())
}
