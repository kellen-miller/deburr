//! HTML rendering with askama templates (`templates/report.html` and
//! `templates/comparison.html`). Templates escape every interpolated value;
//! the only scripts and styles are static text inside the templates.

use std::collections::{BTreeSet, HashMap};
use std::io::{self, Write};

use askama::Template;

use super::{
    Source, Style, clone_text, enum_name, finding_text, function_text, ledger_of, location,
    previous_text, provenance_text, ranked_functions, review_fields,
};
use crate::report::{
    AmbiguousMatch, Category, Change, ChangeState, CodeClone, Comparison, Finding, MetricBucket,
    MetricDelta, Report, ReviewItem, ReviewKind,
};

/// Status shown for inventory rows with no review decision.
const OBSERVED_STATUS: &str = "observed";

struct MetricRow {
    label: String,
    code_lines: String,
    functions: String,
    cyclomatic: String,
    max_nesting: String,
    mass: String,
    high_complexity_functions: String,
    high_complexity_mass: String,
    share: String,
}

impl MetricRow {
    fn bucket(label: &str, bucket: &MetricBucket) -> Self {
        Self {
            label: label.to_owned(),
            code_lines: bucket.code_lines.to_string(),
            functions: bucket.functions.to_string(),
            cyclomatic: bucket.cyclomatic.to_string(),
            max_nesting: bucket.max_nesting.to_string(),
            mass: format!("{:.6}", bucket.mass),
            high_complexity_functions: bucket.high_complexity_functions.to_string(),
            high_complexity_mass: format!("{:.6}", bucket.high_complexity_mass),
            share: bucket
                .high_complexity_mass_share
                .map_or_else(|| "null".to_owned(), |share| format!("{share:.6}")),
        }
    }

    fn delta(label: &str, delta: &MetricDelta) -> Self {
        Self {
            label: label.to_owned(),
            code_lines: format!("{:+}", delta.code_lines),
            functions: format!("{:+}", delta.functions),
            cyclomatic: format!("{:+}", delta.cyclomatic),
            max_nesting: format!("{:+}", delta.max_nesting),
            mass: format!("{:+.6}", delta.mass),
            high_complexity_functions: format!("{:+}", delta.high_complexity_functions),
            high_complexity_mass: format!("{:+.6}", delta.high_complexity_mass),
            share: delta
                .high_complexity_mass_share
                .map_or_else(|| "unavailable".to_owned(), |share| format!("{share:+.6}")),
        }
    }
}

/// One function, finding, or clone in the searchable review inventory.
#[derive(Default)]
struct InventoryRow {
    kind: &'static str,
    category: String,
    status: String,
    id: String,
    fingerprint: String,
    subject: String,
    location: String,
    metrics: String,
    reason_category: String,
    reason: String,
    evidence: String,
    upstream: String,
    previous: String,
    search: String,
    review: bool,
    stale: bool,
}

impl InventoryRow {
    /// A row with no review decision yet.
    fn observed() -> Self {
        Self {
            status: OBSERVED_STATUS.to_owned(),
            ..Self::default()
        }
    }

    /// Copies a review decision onto the row, if the ledger has one.
    fn apply_review(&mut self, item: Option<&&ReviewItem>) {
        let Some(item) = item else {
            return;
        };

        self.status = enum_name(&item.status);
        self.fingerprint.clone_from(&item.fingerprint);
        self.reason.clone_from(&item.reason);
        self.reason_category.clone_from(&item.reason_category);
        self.evidence.clone_from(&item.evidence);
        self.upstream.clone_from(&item.upstream);
        self.previous = item
            .previous
            .as_ref()
            .map(previous_text)
            .unwrap_or_default();
        self.stale = item.stale;
    }

    fn finish(&mut self) {
        self.search = [
            self.kind,
            &self.category,
            &self.status,
            &self.id,
            &self.fingerprint,
            &self.subject,
            &self.location,
            &self.metrics,
            &self.reason_category,
            &self.reason,
            &self.evidence,
            &self.upstream,
            &self.previous,
        ]
        .join(" ");
    }
}

struct CategoryRow {
    category: &'static str,
    files: u32,
    code_lines: u32,
    duplicated_code_lines: u32,
    percentage: String,
}

struct EndpointRow {
    location: String,
    code_lines: u32,
    functions: String,
}

struct CloneRow {
    id: String,
    family_id: String,
    category: &'static str,
    tokens: u32,
    ambiguous: bool,
    endpoints: Vec<EndpointRow>,
}

struct FileRow {
    path: String,
    category: &'static str,
    status: String,
    bytes: u64,
    lines: u32,
    code_lines: u32,
    reason: String,
    error: String,
}

#[derive(Template)]
#[template(path = "report.html")]
struct ReportPage<'a> {
    report: &'a Report,
    analyzer: String,
    scope: String,
    provenance: String,
    metrics: Vec<MetricRow>,
    hotspot_count: usize,
    function_count: usize,
    finding_count: usize,
    clone_count: usize,
    review_counts: &'a crate::report::ReviewCounts,
    rows: Vec<InventoryRow>,
    category_options: Vec<String>,
    status_options: Vec<String>,
    duplication_status: String,
    duplication_categories: Vec<CategoryRow>,
    clones: Vec<CloneRow>,
    files: Vec<FileRow>,
}

/// Writes the HTML report.
///
/// # Errors
///
/// Returns an I/O error when rendering or writing fails.
pub fn report(
    report: &Report,
    _source: &Source,
    _style: Style,
    out: &mut dyn Write,
) -> io::Result<()> {
    let ledger = ledger_of(report);
    let decisions: HashMap<(ReviewKind, &str), &ReviewItem> = ledger
        .items
        .iter()
        .map(|item| ((item.kind, item.id.as_str()), item))
        .collect();
    let category_of_path: HashMap<&str, Category> = report
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.category))
        .collect();

    let mut rows = Vec::new();

    let functions = ranked_functions(&report.functions);
    for function in &functions {
        let signals = &function.structural_signals;
        let mut row = InventoryRow {
            kind: "function",
            category: function.category.as_str().to_owned(),
            id: function.id.clone(),
            subject: function.name.clone(),
            location: location(&function.path, function.start, function.end),
            metrics: format!(
                "sloc={} cyclomatic={} max_nesting={} mass={:.6} flat_guards={} field_mappings={} nested_branches={} assertion_like_calls={} identity_ambiguous={} high_complexity={}",
                function.sloc,
                function.cyclomatic,
                function.max_nesting,
                function.mass,
                signals.flat_guards,
                signals.field_mappings,
                signals.nested_branches,
                signals.assertion_like_calls,
                function.identity_ambiguous,
                function.high_complexity,
            ),
            reason: if function.high_complexity {
                "high_complexity=true".to_owned()
            } else {
                String::new()
            },
            review: function.high_complexity,
            ..InventoryRow::observed()
        };
        if function.high_complexity {
            row.apply_review(decisions.get(&(ReviewKind::Function, function.id.as_str())));
        }

        rows.push(row);
    }

    let mut findings: Vec<&Finding> = report.findings.iter().collect();
    findings.sort_by(|left, right| {
        (&left.path, left.start, &left.rule, &left.id).cmp(&(
            &right.path,
            right.start,
            &right.rule,
            &right.id,
        ))
    });
    for finding in &findings {
        let category = category_of_path
            .get(finding.path.as_str())
            .map_or("unknown", |category| category.as_str());
        let mut row = InventoryRow {
            kind: "finding",
            category: category.to_owned(),
            id: finding.id.clone(),
            subject: format!("{}: {}", finding.rule, finding.message),
            location: location(&finding.path, finding.start, finding.end),
            metrics: format!(
                "severity={} identity_ambiguous={}",
                finding.severity, finding.identity_ambiguous
            ),
            reason: finding.message.clone(),
            review: true,
            ..InventoryRow::observed()
        };
        row.apply_review(decisions.get(&(ReviewKind::Finding, finding.id.as_str())));
        rows.push(row);
    }

    let mut clones: Vec<&CodeClone> = report.duplication.clones.iter().collect();
    clones.sort_by(|left, right| {
        let first = |clone: &CodeClone| {
            clone
                .endpoints
                .first()
                .map(|endpoint| (endpoint.path.clone(), endpoint.start))
        };

        (left.category, first(left), left.tokens, &left.id).cmp(&(
            right.category,
            first(right),
            right.tokens,
            &right.id,
        ))
    });
    for clone in &clones {
        let locations: Vec<String> = clone
            .endpoints
            .iter()
            .map(|endpoint| location(&endpoint.path, endpoint.start, endpoint.end))
            .collect();
        let functions: Vec<String> = clone
            .endpoints
            .iter()
            .map(|endpoint| endpoint.functions.join(","))
            .collect();
        let mut row = InventoryRow {
            kind: "clone",
            category: clone.category.as_str().to_owned(),
            id: clone.id.clone(),
            subject: "duplicate source spans".to_owned(),
            location: locations.join("; "),
            metrics: format!(
                "family_id={} tokens={} identity_ambiguous={} endpoint_functions={}",
                clone.family_id,
                clone.tokens,
                clone.identity_ambiguous,
                functions.join(" | "),
            ),
            review: true,
            ..InventoryRow::observed()
        };
        row.apply_review(decisions.get(&(ReviewKind::Clone, clone.id.as_str())));
        rows.push(row);
    }

    for row in &mut rows {
        row.finish();
    }

    let category_options: BTreeSet<String> = rows.iter().map(|row| row.category.clone()).collect();
    let status_options: BTreeSet<String> = rows.iter().map(|row| row.status.clone()).collect();

    let page = ReportPage {
        report,
        analyzer: format!("{} {}", report.analyzer.id, report.analyzer.version),
        scope: format!(
            "languages={} include_tests={} include_testdata={} include_vendor={} include_generated={}",
            report.scope.languages.join(","),
            report.scope.include_tests,
            report.scope.include_testdata,
            report.scope.include_vendor,
            report.scope.include_generated,
        ),
        provenance: provenance_text(&report.provenance),
        metrics: vec![
            MetricRow::bucket("all", &report.metrics.all),
            MetricRow::bucket("production", &report.metrics.production),
            MetricRow::bucket("test", &report.metrics.test),
        ],
        hotspot_count: functions
            .iter()
            .filter(|function| function.high_complexity)
            .count(),
        function_count: functions.len(),
        finding_count: findings.len(),
        clone_count: clones.len(),
        review_counts: &ledger.counts,
        rows,
        category_options: category_options.into_iter().collect(),
        status_options: status_options.into_iter().collect(),
        duplication_status: enum_name(&report.duplication.status),
        duplication_categories: report
            .duplication
            .categories
            .iter()
            .map(|measured| CategoryRow {
                category: measured.category.as_str(),
                files: measured.files,
                code_lines: measured.code_lines,
                duplicated_code_lines: measured.duplicated_code_lines,
                percentage: format!("{:.2}", measured.percentage),
            })
            .collect(),
        clones: clones
            .iter()
            .map(|clone| CloneRow {
                id: clone.id.clone(),
                family_id: clone.family_id.clone(),
                category: clone.category.as_str(),
                tokens: clone.tokens,
                ambiguous: clone.identity_ambiguous,
                endpoints: clone
                    .endpoints
                    .iter()
                    .map(|endpoint| EndpointRow {
                        location: location(&endpoint.path, endpoint.start, endpoint.end),
                        code_lines: endpoint.code_lines,
                        functions: endpoint.functions.join(", "),
                    })
                    .collect(),
            })
            .collect(),
        files: report
            .files
            .iter()
            .map(|file| FileRow {
                path: file.path.clone(),
                category: file.category.as_str(),
                status: enum_name(&file.status),
                bytes: file.bytes,
                lines: file.lines,
                code_lines: file.code_lines,
                reason: file.reason.clone(),
                error: file.error.clone(),
            })
            .collect(),
    };

    out.write_all(page.render().map_err(io::Error::other)?.as_bytes())
}

struct FindingRow {
    location: String,
    rule: String,
    severity: String,
    message: String,
    id: String,
    ambiguous: bool,
}

impl FindingRow {
    fn new(finding: &Finding) -> Self {
        Self {
            location: location(&finding.path, finding.start, finding.end),
            rule: finding.rule.clone(),
            severity: finding.severity.clone(),
            message: finding.message.clone(),
            id: finding.id.clone(),
            ambiguous: finding.identity_ambiguous,
        }
    }
}

/// One changed function, finding, or clone.
struct ChangeRow {
    state: String,
    basis: String,
    before_ids: String,
    after_ids: String,
    before: String,
    after: String,
    review: String,
    note: bool,
}

impl ChangeRow {
    fn new<T>(change: &Change<T>, describe: fn(&T) -> String) -> Self {
        let review: Vec<String> = [
            ("before", &change.before_review),
            ("after", &change.after_review),
        ]
        .into_iter()
        .filter_map(|(side, item)| {
            item.as_ref().map(|item| {
                format!(
                    "{side} status={}{}",
                    enum_name(&item.status),
                    review_fields(item)
                )
            })
        })
        .collect();

        Self {
            state: enum_name(&change.state),
            basis: change
                .match_basis
                .as_ref()
                .map(enum_name)
                .unwrap_or_default(),
            before_ids: change.before_ids.join(", "),
            after_ids: change.after_ids.join(", "),
            before: change.before.as_ref().map(describe).unwrap_or_default(),
            after: change.after.as_ref().map(describe).unwrap_or_default(),
            review: review.join("; "),
            note: matches!(
                change.state,
                ChangeState::ResolvedThreshold
                    | ChangeState::DeletedFunction
                    | ChangeState::SourceRemoved
            ),
        }
    }
}

struct AmbiguousRow {
    kind: &'static str,
    path: String,
    rule: String,
    message: String,
    before_ids: String,
    after_ids: String,
}

impl AmbiguousRow {
    fn new(kind: &'static str, ambiguous: &AmbiguousMatch) -> Self {
        Self {
            kind,
            path: ambiguous.path.clone(),
            rule: ambiguous.rule.clone(),
            message: ambiguous.message.clone(),
            before_ids: ambiguous.before_ids.join(", "),
            after_ids: ambiguous.after_ids.join(", "),
        }
    }
}

#[derive(Template)]
#[template(path = "comparison.html")]
struct ComparisonPage<'a> {
    comparison: &'a Comparison,
    before_analyzer: String,
    after_analyzer: String,
    before_provenance: String,
    after_provenance: String,
    metrics: Vec<MetricRow>,
    has_delta: bool,
    new_findings: Vec<FindingRow>,
    resolved_findings: Vec<FindingRow>,
    finding_changes: Vec<ChangeRow>,
    function_changes: Vec<ChangeRow>,
    clone_changes: Vec<ChangeRow>,
    ambiguous: Vec<AmbiguousRow>,
    duplication_before_status: String,
    duplication_after_status: String,
    before_clones: Vec<String>,
    after_clones: Vec<String>,
}

/// Writes the HTML comparison.
///
/// # Errors
///
/// Returns an I/O error when rendering or writing fails.
pub fn comparison(
    comparison: &Comparison,
    _source: &Source,
    _style: Style,
    out: &mut dyn Write,
) -> io::Result<()> {
    let mut metrics = Vec::new();
    for (label, before, after) in [
        (
            "all",
            &comparison.before_metrics.all,
            &comparison.after_metrics.all,
        ),
        (
            "production",
            &comparison.before_metrics.production,
            &comparison.after_metrics.production,
        ),
        (
            "test",
            &comparison.before_metrics.test,
            &comparison.after_metrics.test,
        ),
    ] {
        metrics.push(MetricRow::bucket(&format!("Before {label}"), before));
        metrics.push(MetricRow::bucket(&format!("After {label}"), after));
    }

    if let Some(delta) = &comparison.metrics_delta {
        metrics.push(MetricRow::delta("Delta all", &delta.all));
        metrics.push(MetricRow::delta("Delta production", &delta.production));
        metrics.push(MetricRow::delta("Delta test", &delta.test));
    }

    let duplication = &comparison.duplication;
    let page = ComparisonPage {
        comparison,
        before_analyzer: format!(
            "{} {}",
            comparison.before_analyzer.id, comparison.before_analyzer.version
        ),
        after_analyzer: format!(
            "{} {}",
            comparison.after_analyzer.id, comparison.after_analyzer.version
        ),
        before_provenance: provenance_text(&comparison.before_provenance),
        after_provenance: provenance_text(&comparison.after_provenance),
        metrics,
        has_delta: comparison.metrics_delta.is_some(),
        new_findings: comparison
            .new_findings
            .iter()
            .map(FindingRow::new)
            .collect(),
        resolved_findings: comparison
            .resolved_findings
            .iter()
            .map(FindingRow::new)
            .collect(),
        finding_changes: comparison
            .finding_changes
            .iter()
            .map(|change| ChangeRow::new(change, finding_text))
            .collect(),
        function_changes: comparison
            .function_changes
            .iter()
            .map(|change| ChangeRow::new(change, function_text))
            .collect(),
        clone_changes: comparison
            .clone_changes
            .iter()
            .map(|change| ChangeRow::new(change, clone_text))
            .collect(),
        ambiguous: [
            ("finding", &comparison.ambiguous_findings),
            ("function", &comparison.ambiguous_functions),
            ("clone", &comparison.ambiguous_clones),
        ]
        .into_iter()
        .flat_map(|(kind, matches)| matches.iter().map(move |one| AmbiguousRow::new(kind, one)))
        .collect(),
        duplication_before_status: enum_name(&duplication.before_status),
        duplication_after_status: enum_name(&duplication.after_status),
        before_clones: duplication.before_clones.iter().map(clone_text).collect(),
        after_clones: duplication.after_clones.iter().map(clone_text).collect(),
    };

    out.write_all(page.render().map_err(io::Error::other)?.as_bytes())
}
