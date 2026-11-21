//! Argument parsing, command dispatch, exit codes, progress output, and
//! output-file safety.
//!
//! Exit codes: 0 success, 1 failure (including an audit whose report was
//! written but hit analysis errors), 2 usage error.

use std::env;
use std::fs::{self, File};
use std::io::{self, BufWriter, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::{Args, Parser, Subcommand};
use serde::de::DeserializeOwned;

use crate::audit::{self, AuditOptions};
use crate::compare;
use crate::duplication::DuplicationSettings;
use crate::provenance;
use crate::render::{self, Format, Source, Style};
use crate::report::{
    DEFAULT_MIN_CLONE_LINES, DEFAULT_MIN_CLONE_TOKENS, Report, ReviewLedger, ValidationError,
};
use crate::review::{self, ReviewError};

mod workspace_paths;

use workspace_paths::{github_path_prefix, prefix_comparison_paths, prefix_paths};

#[derive(Debug, Parser)]
#[command(
    name = "deburr",
    version = crate::VERSION,
    about = "Deterministic cleanup evidence for Go and TypeScript code",
    subcommand_required = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Audit a source tree and write a report
    Audit(AuditArgs),
    /// Compare two JSON reports
    Compare(CompareArgs),
    /// Create or apply a review ledger
    Review(ReviewArgs),
    /// Render a JSON report in another format
    Render(RenderArgs),
    /// Print a workflow guide
    Guide(GuideArgs),
}

#[derive(Debug, Args)]
struct AuditArgs {
    /// Directory (or file) to audit
    path: PathBuf,
    /// Report format
    #[arg(long, value_enum, ignore_case = true, default_value = "text")]
    format: Format,
    /// Write the report to FILE instead of standard output
    #[arg(long, value_name = "FILE")]
    output: Option<PathBuf>,
    /// Exclude test files
    #[arg(long)]
    exclude_tests: bool,
    /// Include testdata directories
    #[arg(long)]
    include_testdata: bool,
    /// Include vendor directories
    #[arg(long)]
    include_vendor: bool,
    /// Include generated files
    #[arg(long)]
    include_generated: bool,
    /// Skip files larger than N bytes
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u64).range(1..))]
    max_file_bytes: Option<u64>,
    /// Suppress progress lines on standard error
    #[arg(long)]
    quiet: bool,
    /// Accepted for compatibility; duplication is on by default
    #[arg(long)]
    duplicates: bool,
    /// Skip clone detection
    #[arg(long)]
    no_duplicates: bool,
    /// Minimum tokens in a clone
    #[arg(
        long,
        value_name = "N",
        default_value_t = DEFAULT_MIN_CLONE_TOKENS,
        value_parser = clap::value_parser!(u32).range(1..)
    )]
    min_clone_tokens: u32,
    /// Minimum code lines in each clone region
    #[arg(
        long,
        value_name = "N",
        default_value_t = DEFAULT_MIN_CLONE_LINES,
        value_parser = clap::value_parser!(u32).range(1..)
    )]
    min_clone_lines: u32,
}

#[derive(Debug, Args)]
struct CompareArgs {
    /// Report taken before the cleanup
    before: PathBuf,
    /// Report taken after the cleanup
    after: PathBuf,
    /// Output format
    #[arg(long, value_enum, ignore_case = true, default_value = "text")]
    format: Format,
    /// Write the result to FILE instead of standard output
    #[arg(long, value_name = "FILE")]
    output: Option<PathBuf>,
    /// Source tree of the after report, for links and code snippets
    #[arg(long, value_name = "DIR")]
    source: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct RenderArgs {
    /// JSON report to render
    report: PathBuf,
    /// Output format
    #[arg(long, value_enum, ignore_case = true, default_value = "text")]
    format: Format,
    /// Write the result to FILE instead of standard output
    #[arg(long, value_name = "FILE")]
    output: Option<PathBuf>,
    /// Source tree the report was taken from, for links and code snippets
    #[arg(long, value_name = "DIR")]
    source: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct ReviewArgs {
    #[command(subcommand)]
    command: ReviewCommand,
}

#[derive(Debug, Subcommand)]
enum ReviewCommand {
    /// Write an unreviewed ledger for a report
    Init(ReviewInitArgs),
    /// Apply a ledger to a report and render the result
    Apply(ReviewApplyArgs),
}

#[derive(Debug, Args)]
struct ReviewInitArgs {
    /// JSON report to inventory
    report: PathBuf,
    /// Write the ledger to FILE instead of standard output
    #[arg(long, value_name = "FILE")]
    output: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct ReviewApplyArgs {
    /// JSON report to review
    report: PathBuf,
    /// Review ledger to apply
    #[arg(long, value_name = "FILE")]
    ledger: PathBuf,
    /// Output format
    #[arg(long, value_enum, ignore_case = true, default_value = "text")]
    format: Format,
    /// Write the result to FILE instead of standard output
    #[arg(long, value_name = "FILE")]
    output: Option<PathBuf>,
    /// Source tree the report was taken from, for links and code snippets
    #[arg(long, value_name = "DIR")]
    source: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct GuideArgs {
    #[command(subcommand)]
    command: GuideCommand,
}

#[derive(Debug, Subcommand)]
enum GuideCommand {
    /// Print the cleanup workflow guide
    Cleanup,
}

#[derive(Debug, thiserror::Error)]
enum CliError {
    #[error("{0}")]
    Usage(String),
    #[error("{context}: {source}")]
    Io { context: String, source: io::Error },
    #[error("{path}: {source}")]
    Decode {
        path: String,
        source: serde_json::Error,
    },
    #[error("{path}: {source}")]
    Invalid {
        path: String,
        source: ValidationError,
    },
    #[error(transparent)]
    Review(#[from] ReviewError),
}

impl CliError {
    fn io(context: impl Into<String>, source: io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }

    fn exit_code(&self) -> ExitCode {
        match self {
            Self::Usage(_) => ExitCode::from(2),
            _ => ExitCode::FAILURE,
        }
    }
}

/// Runs the command line and returns the process exit code.
pub fn run() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            drop(error.print());

            return ExitCode::from(u8::try_from(error.exit_code()).unwrap_or(2));
        }
    };

    let result = match cli.command {
        Command::Audit(args) => audit_command(&args),
        Command::Compare(args) => compare_command(&args),
        Command::Render(args) => render_command(&args),
        Command::Review(args) => match args.command {
            ReviewCommand::Init(args) => review_init_command(&args),
            ReviewCommand::Apply(args) => review_apply_command(&args),
        },
        Command::Guide(args) => match args.command {
            GuideCommand::Cleanup => {
                emit(None, "write guide", render::text::cleanup_guide).map(|()| ExitCode::SUCCESS)
            }
        },
    };

    match result {
        Ok(code) => code,
        Err(error) => {
            say(&mut io::stderr(), &error.to_string());

            error.exit_code()
        }
    }
}

/// Writes one `deburr: ` diagnostic line. A failed write has nowhere to be
/// reported, so it is dropped.
fn say(out: &mut dyn Write, message: &str) {
    drop(writeln!(out, "deburr: {message}"));
}

fn audit_command(args: &AuditArgs) -> Result<ExitCode, CliError> {
    if let Some(output) = &args.output {
        validate_output(output, &[&args.path])?;
    }

    let options = AuditOptions {
        include_tests: !args.exclude_tests,
        include_testdata: args.include_testdata,
        include_vendor: args.include_vendor,
        include_generated: args.include_generated,
        max_file_bytes: args.max_file_bytes,
        duplication: (!args.no_duplicates).then(|| DuplicationSettings {
            min_tokens: usize::try_from(args.min_clone_tokens).unwrap_or(usize::MAX),
            min_lines: usize::try_from(args.min_clone_lines).unwrap_or(usize::MAX),
        }),
    };

    let mut stderr = io::stderr();
    let mut sink = io::sink();
    let progress: &mut dyn Write = if args.quiet { &mut sink } else { &mut stderr };

    let mut outcome = audit::audit(&args.path, &options, progress);

    if args.format == Format::Github
        && let Some(prefix) = github_path_prefix(&args.path)
    {
        prefix_paths(&mut outcome.report, &prefix);
    }

    let root = if args.path.is_file() {
        args.path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
    } else {
        &args.path
    };
    let (source, style) = render_context(
        Some(root),
        &outcome.report,
        args.format,
        args.output.is_none(),
    );

    say(progress, &format!("writing {} report", args.format.name()));
    let written = emit(args.output.as_deref(), "write report", |out| {
        render::render_report(&outcome.report, &source, style, args.format, out)
    });

    if !outcome.errors.is_empty() {
        let messages: Vec<String> = outcome.errors.iter().map(ToString::to_string).collect();
        say(
            &mut io::stderr(),
            &format!("analysis failed: {}", messages.join("; ")),
        );
        written?;

        return Ok(ExitCode::FAILURE);
    }

    written?;
    say(progress, "audit complete");

    Ok(ExitCode::SUCCESS)
}

fn compare_command(args: &CompareArgs) -> Result<ExitCode, CliError> {
    if let Some(output) = &args.output {
        validate_output(output, &[&args.before, &args.after])?;
    }

    let before: Report = read_report(&args.before)?;
    let after: Report = read_report(&args.after)?;
    let mut comparison = compare::compare(&before, &after);
    let (source, style) = render_context(
        verified_source(args.source.as_deref(), &after),
        &after,
        args.format,
        args.output.is_none(),
    );

    if args.format == Format::Github
        && let Some(prefix) = github_path_prefix(args.source.as_deref().unwrap_or(Path::new(".")))
    {
        prefix_comparison_paths(&mut comparison, &prefix);
    }

    emit(args.output.as_deref(), "write comparison", |out| {
        render::render_comparison(&comparison, &source, style, args.format, out)
    })?;

    if !comparison.compatible || !comparison.complete {
        say(
            &mut io::stderr(),
            "reports are incompatible or incomplete; no improvement conclusion was made",
        );

        return Ok(ExitCode::FAILURE);
    }

    Ok(ExitCode::SUCCESS)
}

fn review_init_command(args: &ReviewInitArgs) -> Result<ExitCode, CliError> {
    if let Some(output) = &args.output {
        validate_output(output, &[&args.report])?;
    }

    let report = read_report(&args.report)?;
    let ledger = review::init_ledger(&report);

    emit(args.output.as_deref(), "write ledger", |out| {
        render::write_json(&ledger, out)
    })?;

    Ok(ExitCode::SUCCESS)
}

fn review_apply_command(args: &ReviewApplyArgs) -> Result<ExitCode, CliError> {
    if let Some(output) = &args.output {
        validate_output(output, &[&args.report, &args.ledger])?;
    }

    let report = read_report(&args.report)?;
    let ledger: ReviewLedger = read_json(&args.ledger)?;
    let mut reviewed = review::apply_ledger(&report, &ledger)?.0;
    let (source, style) = render_context(
        verified_source(args.source.as_deref(), &reviewed),
        &reviewed,
        args.format,
        args.output.is_none(),
    );

    if args.format == Format::Github
        && let Some(prefix) = github_path_prefix(args.source.as_deref().unwrap_or(Path::new(".")))
    {
        prefix_paths(&mut reviewed, &prefix);
    }

    emit(args.output.as_deref(), "write report", |out| {
        render::render_report(&reviewed, &source, style, args.format, out)
    })?;

    Ok(ExitCode::SUCCESS)
}

fn render_command(args: &RenderArgs) -> Result<ExitCode, CliError> {
    if let Some(output) = &args.output {
        validate_output(output, &[&args.report])?;
    }

    let mut report = read_report(&args.report)?;
    let (source, style) = render_context(
        verified_source(args.source.as_deref(), &report),
        &report,
        args.format,
        args.output.is_none(),
    );
    let source = source.named(report_name(args.source.as_deref(), Some(&args.report)));

    if args.format == Format::Github
        && let Some(prefix) = github_path_prefix(args.source.as_deref().unwrap_or(Path::new(".")))
    {
        prefix_paths(&mut report, &prefix);
    }

    emit(args.output.as_deref(), "write report", |out| {
        render::render_report(&report, &source, style, args.format, out)
    })?;

    Ok(ExitCode::SUCCESS)
}

/// The audited directory's name, else the report file's stem, else `report`.
fn report_name(root: Option<&Path>, report_file: Option<&Path>) -> String {
    root.and_then(|root| fs::canonicalize(root).ok())
        .and_then(|root| {
            root.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .or_else(|| {
            report_file
                .and_then(Path::file_stem)
                .map(|stem| stem.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "report".to_owned())
}

/// Where code lives and whether color is allowed, for one render.
///
/// `root` is the directory report paths are relative to. Links need a clean
/// checkout with a known commit and a GitHub origin (or the Actions
/// environment); without them locations print as plain `file:line`.
fn render_context(
    root: Option<&Path>,
    report: &Report,
    format: Format,
    to_stdout: bool,
) -> (Source, Style) {
    let color = to_stdout
        && format == Format::Text
        && io::stdout().is_terminal()
        && env::var_os("NO_COLOR").is_none_or(|value| value.is_empty());
    let style = Style { color };

    let origin = &report.provenance.source;
    let mut link_base = None;
    let mut path_prefix = String::new();

    if origin.git_known
        && !origin.dirty
        && !origin.commit.is_empty()
        && let Some(root) = root
    {
        let actions = env::var("GITHUB_ACTIONS").is_ok_and(|value| value == "true");
        let hosted = env::var("GITHUB_SERVER_URL")
            .ok()
            .zip(env::var("GITHUB_REPOSITORY").ok())
            .filter(|(server, repository)| actions && !server.is_empty() && !repository.is_empty());

        if let Some((server, repository)) = hosted {
            link_base = Some(format!(
                "{}/{repository}/blob/{}",
                server.trim_end_matches('/'),
                origin.commit
            ));
            path_prefix = github_path_prefix(root).unwrap_or_default();
        } else {
            link_base = provenance::github_blob_base(root, &origin.commit);
            path_prefix = provenance::repository_prefix(root).unwrap_or_default();
        }
    }

    // GitHub-format reports are rewritten to workspace-relative paths first.
    if format == Format::Github {
        path_prefix.clear();
    }

    if !path_prefix.is_empty() && !path_prefix.ends_with('/') {
        path_prefix.push('/');
    }

    let source = Source::new(root.map(Path::to_path_buf), link_base, path_prefix)
        .named(report_name(root, None));

    (source, style)
}

/// `dir` when it provably holds the code `report` describes: the report
/// records a clean known commit and `dir` is a clean checkout of it.
/// Otherwise warns and returns `None`, so only locations are rendered.
fn verified_source<'a>(dir: Option<&'a Path>, report: &Report) -> Option<&'a Path> {
    let dir = dir?;
    let origin = &report.provenance.source;

    if !origin.git_known || origin.dirty || origin.commit.is_empty() {
        say(
            &mut io::stderr(),
            &format!(
                "--source {} ignored: report records no clean git commit; showing locations only",
                dir.display()
            ),
        );

        return None;
    }

    let matches = provenance::source(dir, "")
        .is_ok_and(|actual| !actual.dirty && actual.commit == origin.commit);

    if !matches {
        say(
            &mut io::stderr(),
            &format!(
                "--source {} does not match the report's commit; showing locations only",
                dir.display()
            ),
        );

        return None;
    }

    Some(dir)
}

/// Decodes one JSON value, rejecting unknown fields and trailing data.
fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, CliError> {
    let bytes = fs::read(path)
        .map_err(|source| CliError::io(format!("read {}", path.display()), source))?;

    serde_json::from_slice(&bytes).map_err(|source| CliError::Decode {
        path: path.display().to_string(),
        source,
    })
}

fn read_report(path: &Path) -> Result<Report, CliError> {
    let report: Report = read_json(path)?;

    report.validate().map_err(|source| CliError::Invalid {
        path: path.display().to_string(),
        source,
    })?;

    Ok(report)
}

/// Rejects output paths that would clobber an input, land inside the audited
/// tree, or are not plain files.
fn validate_output(output: &Path, inputs: &[&Path]) -> Result<(), CliError> {
    if output.as_os_str() == "-" {
        return Err(CliError::Usage(
            "--output - is not supported; omit --output for standard output".to_owned(),
        ));
    }

    if let Ok(metadata) = fs::symlink_metadata(output) {
        if metadata.file_type().is_symlink() {
            return Err(CliError::Usage(format!(
                "output {} is a symlink",
                output.display()
            )));
        }

        if metadata.is_dir() {
            return Err(CliError::Usage(format!(
                "output {} is a directory",
                output.display()
            )));
        }
    }

    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let Some(name) = output.file_name() else {
        return Err(CliError::Usage(format!(
            "output {} has no file name",
            output.display()
        )));
    };
    let resolved = fs::canonicalize(parent)
        .map_err(|source| {
            CliError::io(
                format!("resolve output directory {}", parent.display()),
                source,
            )
        })?
        .join(name);

    for input in inputs {
        if let Ok(canonical) = fs::canonicalize(input)
            && resolved.starts_with(&canonical)
        {
            return Err(CliError::Usage(format!(
                "output {} is inside or equal to input {}",
                output.display(),
                input.display()
            )));
        }
    }

    Ok(())
}

/// Runs `write` against standard output, or against a temporary file beside
/// `output` that is renamed into place only if writing succeeds.
fn emit(
    output: Option<&Path>,
    what: &str,
    write: impl FnOnce(&mut dyn Write) -> io::Result<()>,
) -> Result<(), CliError> {
    let Some(path) = output else {
        let mut out = BufWriter::new(io::stdout().lock());

        return write(&mut out)
            .and_then(|()| out.flush())
            .map_err(|source| CliError::io(what, source));
    };

    let dir = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let temp = dir.join(format!(".deburr-report-{}-{nanos}", std::process::id()));

    let file = File::create_new(&temp)
        .map_err(|source| CliError::io(format!("create {}", temp.display()), source))?;
    let mut out = BufWriter::new(file);
    let written = write(&mut out).and_then(|()| out.flush());
    drop(out);

    if let Err(source) = written.and_then(|()| fs::rename(&temp, path)) {
        drop(fs::remove_file(&temp));

        return Err(CliError::io(
            format!("{what} to {}", path.display()),
            source,
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn cli_definition_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn prefix_rewrites_every_report_path() {
        let mut report = audit::empty_report(&AuditOptions {
            include_tests: true,
            include_testdata: false,
            include_vendor: false,
            include_generated: false,
            max_file_bytes: None,
            duplication: None,
        });
        report.files.push(crate::report::File {
            path: "a.go".to_owned(),
            category: crate::report::Category::Production,
            status: crate::report::FileStatus::Analyzed,
            reason: String::new(),
            error: String::new(),
            bytes: 1,
            lines: 1,
            code_lines: 1,
        });

        prefix_paths(&mut report, "sub/dir");

        assert_eq!(report.files[0].path, "sub/dir/a.go");
    }

    #[test]
    fn prefix_rewrites_every_comparison_path() {
        use crate::report::{
            Change, ChangeState, CloneEndpoint, CodeClone, Finding, Function, Location, Position,
            ReviewItem,
        };

        let report = audit::empty_report(&AuditOptions {
            include_tests: true,
            include_testdata: false,
            include_vendor: false,
            include_generated: false,
            max_file_bytes: None,
            duplication: None,
        });
        let mut comparison = compare::compare(&report, &report);
        let position = Position { line: 1, column: 1 };
        let function: Function = serde_json::from_value(serde_json::json!({
            "id": "f", "path": "a.go", "category": "production", "name": "f",
            "structural_signals": serde_json::to_value(
                crate::report::StructuralSignals::default()
            ).expect("signals"),
            "start": {"line": 1, "column": 1}, "end": {"line": 2, "column": 1},
            "sloc": 1, "cyclomatic": 1, "max_nesting": 0, "mass": 1.0,
            "high_complexity": false
        }))
        .expect("function");
        let finding = Finding {
            id: "g".to_owned(),
            fingerprint: String::new(),
            rule: "r".to_owned(),
            path: "b.go".to_owned(),
            severity: "warn".to_owned(),
            message: "m".to_owned(),
            details: Vec::new(),
            start: position,
            end: position,
            identity_ambiguous: false,
        };
        let endpoint = |path: &str| CloneEndpoint {
            path: path.to_owned(),
            start: position,
            end: position,
            code_lines: 1,
            functions: Vec::new(),
        };
        let clone = CodeClone {
            id: "c".to_owned(),
            family_id: "c".to_owned(),
            fingerprint: String::new(),
            category: crate::report::Category::Production,
            tokens: 1,
            identity_ambiguous: false,
            endpoints: [endpoint("c.go"), endpoint("d.go")],
        };
        let item = ReviewItem {
            previous: None,
            id: "i".to_owned(),
            kind: crate::report::ReviewKind::Finding,
            fingerprint: String::new(),
            status: crate::report::ReviewStatus::Unreviewed,
            reason_category: String::new(),
            reason: String::new(),
            evidence: String::new(),
            upstream: String::new(),
            locations: vec![Location {
                path: "e.go".to_owned(),
                start: position,
                end: position,
            }],
            stale: false,
            identity_ambiguous: false,
        };

        comparison.function_changes.push(Change {
            state: ChangeState::New,
            match_basis: None,
            before: None,
            after: Some(function),
            before_review: None,
            after_review: None,
            before_ids: Vec::new(),
            after_ids: Vec::new(),
        });
        comparison.new_findings.push(finding);
        comparison.duplication.after_clones.push(clone);
        comparison.after_review.items.push(item);

        prefix_comparison_paths(&mut comparison, "sub/dir");

        assert_eq!(
            comparison.function_changes[0]
                .after
                .as_ref()
                .expect("after")
                .path,
            "sub/dir/a.go"
        );
        assert_eq!(comparison.new_findings[0].path, "sub/dir/b.go");

        let endpoints = &comparison.duplication.after_clones[0].endpoints;

        assert_eq!(endpoints[0].path, "sub/dir/c.go");
        assert_eq!(endpoints[1].path, "sub/dir/d.go");
        assert_eq!(
            comparison.after_review.items[0].locations[0].path,
            "sub/dir/e.go"
        );
    }
}
