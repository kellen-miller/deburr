//! Runs one audit from start to finish and returns its report.

use std::io::Write;
use std::ops::Range;
use std::path::Path;

use rayon::prelude::*;

use crate::analysis::{self, FunctionBody};
use crate::discover::{self, DiscoveredFile, Discovery};
use crate::duplication::{self, AnalyzedFile, DuplicationError, DuplicationSettings};
use crate::identity::{self, ManifestEntry};
use crate::provenance;
use crate::report::{
    ANALYZER_ID, AnalyzerIdentity, Category, CategoryCoverage, ConfigSummary, Coverage,
    DEFAULT_MIN_CLONE_LINES, DEFAULT_MIN_CLONE_TOKENS, Duplication, DuplicationConfig,
    DuplicationStatus, File, FileStatus, Finding, Function, HIGH_COMPLEXITY_THRESHOLD,
    MetricBucket, Metrics, Provenance, Report, RuleConfig, SCHEMA_VERSION, Scope, SourceProvenance,
};
use crate::review;
use crate::syntax::{self, Language, Token};

/// What an audit includes.
#[derive(Debug, Clone)]
pub struct AuditOptions {
    pub include_tests: bool,
    pub include_testdata: bool,
    pub include_vendor: bool,
    pub include_generated: bool,
    pub max_file_bytes: Option<u64>,
    /// `None` means `--no-duplicates`.
    pub duplication: Option<DuplicationSettings>,
}

/// A problem that makes the audit exit 1 after its report is written.
#[derive(Debug, thiserror::Error)]
pub enum AuditError {
    #[error("{path}: {message}")]
    File { path: String, message: String },
    #[error("{0}")]
    Input(String),
    #[error("duplication: {0}")]
    Duplication(String),
}

/// An audit always yields a report; non-empty `errors` means exit 1.
#[derive(Debug)]
pub struct AuditOutcome {
    pub report: Report,
    pub errors: Vec<AuditError>,
}

/// One file after parsing: its report entry, the bytes it was read from (for
/// the manifest digest), and what analysis found in it.
struct FileOutcome {
    file: File,
    source: Vec<u8>,
    functions: Vec<Function>,
    bodies: Vec<FunctionBody>,
    findings: Vec<Finding>,
    /// What clone detection needs, kept only for analyzed production and test
    /// files when duplication is requested (the syntax tree is dropped).
    /// A production file's `tokens` and `code_lines` leave out its inline
    /// test code, which is kept apart below so it can join the test group.
    language: Option<Language>,
    tokens: Vec<Token>,
    /// Runs of `tokens` that were contiguous in the source; clone detection
    /// never matches across two runs.
    segments: Vec<Range<usize>>,
    code_lines: Vec<bool>,
    /// Inline test tokens, their runs, and the lines they mark; empty without
    /// inline tests, and with `--exclude-tests`.
    inline_test_tokens: Vec<Token>,
    inline_test_segments: Vec<Range<usize>>,
    inline_test_lines: Vec<bool>,
}

/// Audits the tree at `root`: discovers files, parses and analyzes them in
/// parallel, then sorts everything by path so output never depends on thread
/// timing. Progress lines go to `progress`; a failed progress write is dropped.
///
/// The report is always returned. `errors` lists unreadable and unparsable
/// files (and an unreadable `root`); the caller exits 1 after writing it.
pub fn audit(root: &Path, options: &AuditOptions, progress: &mut dyn Write) -> AuditOutcome {
    let mut report = empty_report(options);
    let mut errors = Vec::new();

    say(progress, "scanning Go, TypeScript, and Rust source");

    let discovery = discover::discover(root, options).unwrap_or_else(|error| {
        errors.push(AuditError::Input(error.to_string()));

        Discovery::default()
    });
    let symlinks = discovery.symlinks;
    let excluded_directories = discovery.excluded_directories;

    let keep_tokens = options.duplication.is_some();
    let mut outcomes: Vec<FileOutcome> = discovery
        .files
        .into_par_iter()
        .map(|file| analyze_file(file, keep_tokens, options.include_tests))
        .collect();
    outcomes.sort_by(|left, right| left.file.path.cmp(&right.file.path));

    // Coverage and progress are tallied in path order after the parallel
    // phase, so both are deterministic.
    let mut by_category: Vec<CategoryCoverage> = Category::ALL
        .iter()
        .map(|&category| CategoryCoverage {
            category,
            discovered: 0,
            analyzed: 0,
            excluded: 0,
            unsupported: 0,
            read_errors: 0,
            parse_errors: 0,
        })
        .collect();
    let mut analyzed = 0_u32;
    let mut excluded = 0_u32;
    let mut unsupported = 0_u32;
    let mut failed = 0_u32;

    for (index, outcome) in outcomes.iter().enumerate() {
        let file = &outcome.file;

        if let Some(row) = by_category
            .iter_mut()
            .find(|row| row.category == file.category)
        {
            row.discovered += 1;

            match file.status {
                FileStatus::Analyzed => row.analyzed += 1,
                FileStatus::Excluded => row.excluded += 1,
                FileStatus::Unsupported => row.unsupported += 1,
                FileStatus::ReadError => row.read_errors += 1,
                FileStatus::ParseError => row.parse_errors += 1,
            }
        }

        match file.status {
            FileStatus::Analyzed => analyzed += 1,
            FileStatus::Excluded => excluded += 1,
            FileStatus::Unsupported => unsupported += 1,
            FileStatus::ReadError | FileStatus::ParseError => {
                failed += 1;
                errors.push(AuditError::File {
                    path: file.path.clone(),
                    message: file.error.clone(),
                });
            }
        }

        let discovered = index + 1;

        if discovered == 1 || discovered % 100 == 0 {
            say(
                progress,
                &format!(
                    "scanned {discovered} files ({analyzed} analyzed, {excluded} excluded, \
                     {unsupported} unsupported, {failed} errors)"
                ),
            );
        }
    }

    say(
        progress,
        &format!(
            "scan finished: {} files, {analyzed} analyzed, {failed} errors",
            outcomes.len()
        ),
    );

    if let Some(settings) = &options.duplication {
        say(progress, "detecting clones");

        if let Err(error) = measure_duplication(&outcomes, settings, &mut report.duplication) {
            report.duplication.status = DuplicationStatus::Error;
            report.duplication.error = error.to_string();
            errors.push(AuditError::Duplication(error.to_string()));
        }
    }

    say(progress, "summarizing metrics");

    report.coverage = Coverage {
        discovered: by_category.iter().map(|row| row.discovered).sum(),
        analyzed,
        excluded,
        unsupported,
        read_errors: by_category.iter().map(|row| row.read_errors).sum(),
        parse_errors: by_category.iter().map(|row| row.parse_errors).sum(),
        by_category,
        symlinks,
        excluded_directories,
    };

    let manifest_entries: Vec<ManifestEntry<'_>> = outcomes
        .iter()
        .map(|outcome| ManifestEntry {
            path: &outcome.file.path,
            source: &outcome.source,
        })
        .collect();
    let manifest_digest = identity::manifest_digest(&manifest_entries);

    let mut functions = Vec::new();
    let mut findings = Vec::new();
    let mut files = Vec::with_capacity(outcomes.len());

    for outcome in outcomes {
        functions.extend(outcome.functions);
        findings.extend(outcome.findings);
        files.push(outcome.file);
    }

    functions.sort_by(|left, right| {
        (&left.path, left.start, &left.name, &left.id).cmp(&(
            &right.path,
            right.start,
            &right.name,
            &right.id,
        ))
    });
    findings.sort_by(|left, right| {
        (&left.path, left.start, &left.rule, &left.id).cmp(&(
            &right.path,
            right.start,
            &right.rule,
            &right.id,
        ))
    });

    report.metrics = summarize_metrics(&files, &functions, options.include_tests);
    report.files = files;
    report.functions = functions;
    report.findings = findings;

    say(progress, "collecting source provenance");

    let source_dir = if root.is_dir() {
        root
    } else {
        root.parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
    };

    // Outside a git checkout (or without git) the commit is simply unknown.
    let source =
        provenance::source(source_dir, &manifest_digest).unwrap_or_else(|_| SourceProvenance {
            manifest_digest,
            ..SourceProvenance::default()
        });
    report.provenance = Provenance {
        source,
        build: provenance::build(),
    };
    report.review = Some(review::init_ledger(&report));

    AuditOutcome { report, errors }
}

/// Reads, parses, and analyzes one discovered file. Runs on a rayon worker.
///
/// Every file that was read is tokenized for line counts, including files
/// excluded by category flags and files with syntax errors; only files whose
/// status is still `analyzed` and that parse cleanly are analyzed further.
fn analyze_file(
    discovered: DiscoveredFile,
    keep_tokens: bool,
    include_inline_tests: bool,
) -> FileOutcome {
    let mut outcome = FileOutcome {
        file: File {
            path: discovered.path,
            category: discovered.category,
            status: discovered.status,
            reason: discovered.reason,
            error: discovered.error,
            bytes: discovered.bytes,
            lines: 0,
            code_lines: 0,
            inline_test_code_lines: 0,
        },
        source: Vec::new(),
        functions: Vec::new(),
        bodies: Vec::new(),
        findings: Vec::new(),
        language: None,
        tokens: Vec::new(),
        segments: Vec::new(),
        code_lines: Vec::new(),
        inline_test_tokens: Vec::new(),
        inline_test_segments: Vec::new(),
        inline_test_lines: Vec::new(),
    };

    let (Some(language), Some(source)) = (discovered.language, discovered.source) else {
        return outcome;
    };

    let analyzable = outcome.file.status == FileStatus::Analyzed;
    outcome.language = Some(language);
    outcome.file.lines = u32::try_from(syntax::line_count(&source)).unwrap_or(u32::MAX);

    let (recovered, syntax_error) = match syntax::parse(language, source) {
        Ok(parsed) => (Ok(parsed), None),
        Err(error) => {
            let message = error.to_string();

            (error.recovered(), Some(message))
        }
    };

    if analyzable && let Some(message) = &syntax_error {
        outcome.file.status = FileStatus::ParseError;
        outcome.file.error.clone_from(message);
    }

    let code_lines = match recovered {
        Ok(parsed) => {
            let clean = analyzable && syntax_error.is_none();
            let mut test_regions = Vec::new();
            let code_lines = if clean {
                let analysis = analysis::analyze(
                    &outcome.file.path,
                    outcome.file.category,
                    &parsed,
                    include_inline_tests,
                );
                outcome.functions = analysis.functions;
                outcome.bodies = analysis.bodies;
                outcome.findings = analysis.findings;
                outcome.file.inline_test_code_lines = analysis.inline_test_code_lines;
                test_regions = analysis.test_regions;

                analysis.code_lines
            } else {
                parsed.code_lines()
            };

            let retain = clean
                && keep_tokens
                && matches!(outcome.file.category, Category::Production | Category::Test);

            if retain {
                // Production and inline test code are separate duplication
                // inputs, so each keeps only its own tokens and lines. Real
                // source positions stay on every token. Each input is split
                // into runs that were contiguous in the source, so removing
                // the other category never joins the code around it.
                let inline_lines = parsed.region_code_lines(&test_regions);
                let mut production = Vec::new();
                let mut inline = Vec::new();
                let mut in_inline = false;

                for token in parsed.tokens {
                    let is_inline = token.within(&test_regions);
                    let (tokens, segments) = if is_inline {
                        (&mut inline, &mut outcome.inline_test_segments)
                    } else {
                        (&mut production, &mut outcome.segments)
                    };

                    if is_inline != in_inline || segments.is_empty() {
                        segments.push(tokens.len()..tokens.len());
                    }

                    in_inline = is_inline;
                    tokens.push(token);

                    if let Some(run) = segments.last_mut() {
                        run.end = tokens.len();
                    }
                }

                outcome.tokens = production;
                outcome.code_lines = code_lines
                    .iter()
                    .zip(&inline_lines)
                    .map(|(&code, &inline)| code && !inline)
                    .collect();

                if include_inline_tests && !test_regions.is_empty() {
                    outcome.inline_test_tokens = inline;
                    outcome.inline_test_lines = inline_lines;
                } else {
                    outcome.inline_test_segments.clear();
                }
            }

            outcome.source = parsed.source;

            code_lines
        }
        Err(source) => {
            outcome.source = source;

            Vec::new()
        }
    };

    outcome.file.code_lines =
        u32::try_from(code_lines.iter().filter(|&&code| code).count()).unwrap_or(u32::MAX);

    outcome
}

/// Detects clones separately in production code and in test code, so a clone
/// never pairs production code with test code and each category gets its own
/// percentage. A production file's inline test code joins the test group
/// beside the test files; the rest of the file stays in the production group.
fn measure_duplication(
    outcomes: &[FileOutcome],
    settings: &DuplicationSettings,
    duplication: &mut Duplication,
) -> Result<(), DuplicationError> {
    let mut categories = Vec::new();
    let mut clones = Vec::new();

    for category in [Category::Production, Category::Test] {
        let mut files: Vec<AnalyzedFile<'_>> = Vec::new();

        for outcome in outcomes
            .iter()
            .filter(|outcome| outcome.file.status == FileStatus::Analyzed)
        {
            let Some(language) = outcome.language else {
                continue;
            };
            let inline = category == Category::Test
                && outcome.file.category == Category::Production
                && !outcome.inline_test_tokens.is_empty();

            if outcome.file.category != category && !inline {
                continue;
            }

            files.push(AnalyzedFile {
                path: &outcome.file.path,
                language,
                source: &outcome.source,
                tokens: if inline {
                    &outcome.inline_test_tokens
                } else {
                    &outcome.tokens
                },
                segments: if inline {
                    &outcome.inline_test_segments
                } else {
                    &outcome.segments
                },
                code_lines: if inline {
                    &outcome.inline_test_lines
                } else {
                    &outcome.code_lines
                },
                functions: &outcome.functions,
                bodies: &outcome.bodies,
            });
        }

        let measured = duplication::detect(&files, category, settings)?;

        categories.push(measured.measurement);
        clones.extend(measured.clones);
    }

    clones.sort_by(|left, right| left.id.cmp(&right.id));
    duplication.status = DuplicationStatus::Measured;
    duplication.categories = categories;
    duplication.clones = clones;

    Ok(())
}

/// Code lines of analyzed files and function totals per bucket. Sums follow
/// the (sorted) function order so floating-point addition is deterministic.
///
/// A production file's inline test lines count as test lines. Without
/// `include_tests` they count in no bucket, `all` included.
fn summarize_metrics(files: &[File], functions: &[Function], include_tests: bool) -> Metrics {
    let mut metrics = Metrics::default();

    for file in files
        .iter()
        .filter(|file| file.status == FileStatus::Analyzed)
    {
        let inline = file.inline_test_code_lines;
        let inline_counted = if include_tests { inline } else { 0 };

        metrics.all.code_lines += file.code_lines - inline + inline_counted;

        match file.category {
            Category::Production => {
                metrics.production.code_lines += file.code_lines - inline;
                metrics.test.code_lines += inline_counted;
            }
            Category::Test => metrics.test.code_lines += file.code_lines,
            _ => {}
        }
    }

    for function in functions {
        add_function(&mut metrics.all, function);

        match function.category {
            Category::Production => add_function(&mut metrics.production, function),
            Category::Test => add_function(&mut metrics.test, function),
            _ => {}
        }
    }

    for bucket in [&mut metrics.all, &mut metrics.production, &mut metrics.test] {
        bucket.high_complexity_mass_share =
            (bucket.mass != 0.0).then(|| bucket.high_complexity_mass / bucket.mass);
    }

    metrics
}

fn add_function(bucket: &mut MetricBucket, function: &Function) {
    bucket.functions += 1;
    bucket.cyclomatic += function.cyclomatic;
    bucket.max_nesting = bucket.max_nesting.max(function.max_nesting);
    bucket.mass += function.mass;

    if function.high_complexity {
        bucket.high_complexity_functions += 1;
        bucket.high_complexity_mass += function.mass;
    }
}

/// Writes one `deburr: ` progress line. A failed write has nowhere to be
/// reported, so it is dropped.
fn say(progress: &mut dyn Write, message: &str) {
    drop(writeln!(progress, "deburr: {message}"));
}

pub(crate) fn empty_report(options: &AuditOptions) -> Report {
    let duplication_config = options.duplication.map_or(
        DuplicationConfig {
            requested: false,
            min_tokens: DEFAULT_MIN_CLONE_TOKENS,
            min_lines: DEFAULT_MIN_CLONE_LINES,
        },
        |settings| DuplicationConfig {
            requested: true,
            min_tokens: u32::try_from(settings.min_tokens).unwrap_or(u32::MAX),
            min_lines: u32::try_from(settings.min_lines).unwrap_or(u32::MAX),
        },
    );

    Report {
        duplication: Duplication {
            status: DuplicationStatus::NotRequested,
            error: String::new(),
            min_tokens: duplication_config.min_tokens,
            min_lines: duplication_config.min_lines,
            categories: Vec::new(),
            clones: Vec::new(),
        },
        review: None,
        provenance: Provenance::default(),
        analyzer: AnalyzerIdentity {
            id: ANALYZER_ID.to_owned(),
            version: crate::VERSION.to_owned(),
        },
        schema_version: SCHEMA_VERSION.to_owned(),
        files: Vec::new(),
        functions: Vec::new(),
        findings: Vec::new(),
        scope: Scope {
            path_mode: "source-relative".to_owned(),
            languages: vec!["go".to_owned(), "typescript".to_owned(), "rust".to_owned()],
            include_tests: options.include_tests,
            include_testdata: options.include_testdata,
            include_vendor: options.include_vendor,
            include_generated: options.include_generated,
        },
        metrics: Metrics::default(),
        config: ConfigSummary {
            rules: ["redundant-boolean-return", "duplicate-branches"]
                .into_iter()
                .map(|id| RuleConfig {
                    id: id.to_owned(),
                    enabled: true,
                })
                .collect(),
            language: "go,typescript,rust".to_owned(),
            duplication: duplication_config,
            max_file_bytes: options.max_file_bytes,
            high_complexity_threshold: HIGH_COMPLEXITY_THRESHOLD,
            include_tests: options.include_tests,
            include_testdata: options.include_testdata,
            include_vendor: options.include_vendor,
            include_generated: options.include_generated,
        },
        coverage: Coverage::default(),
    }
}
