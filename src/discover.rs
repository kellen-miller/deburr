//! Walks the audited tree and decides each file's category and status.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::audit::AuditOptions;
use crate::report::{Category, FileStatus};
use crate::syntax::Language;

/// Directories never entered, matched by name at any depth.
const SKIPPED_DIRECTORIES: [&str; 8] = [
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    ".venv",
    "venv",
    ".agent",
    ".worktrees",
];

/// One file found under the audited root.
#[derive(Debug)]
pub struct DiscoveredFile {
    /// Slash-separated path relative to the audited root.
    pub path: String,
    pub category: Category,
    pub status: FileStatus,
    pub reason: String,
    /// Why a read failed; empty otherwise.
    pub error: String,
    pub language: Option<Language>,
    /// Size on disk, or the length of `source` when it was read.
    pub bytes: u64,
    /// Source bytes; `None` for files that were not read.
    pub source: Option<Vec<u8>>,
}

/// The walk result: files sorted by path plus directory and symlink counts.
#[derive(Debug, Default)]
pub struct Discovery {
    pub files: Vec<DiscoveredFile>,
    pub symlinks: u32,
    pub excluded_directories: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum DiscoverError {
    #[error("stat input: {path}: {message}")]
    Input { path: String, message: String },
}

/// Walks `root`, never following symlinks. `root` may be a directory, a
/// single file, or a symlink (which is recorded as excluded).
///
/// Files that cannot be read are returned with status `read_error` rather
/// than failing the walk.
///
/// # Errors
///
/// Returns an error when `root` itself cannot be inspected.
pub fn discover(root: &Path, options: &AuditOptions) -> Result<Discovery, DiscoverError> {
    let absolute = std::path::absolute(root).unwrap_or_else(|_| root.to_path_buf());
    let root_info = fs::symlink_metadata(&absolute).map_err(|error| DiscoverError::Input {
        path: absolute.display().to_string(),
        message: io_message(&error),
    })?;

    let mut discovery = Discovery::default();
    let root_name = absolute
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();

    if root_info.file_type().is_symlink() {
        discovery.record_symlink(root_name, root_info.len());
    } else if root_info.is_file() {
        let file = visit_file(&absolute, root_name, root_info.len(), options);
        discovery.files.push(file);
    } else if root_info.is_dir() {
        let mut pending: Vec<(PathBuf, String)> = vec![(absolute, String::new())];

        while let Some((directory, relative)) = pending.pop() {
            discovery.scan_directory(&directory, &relative, options, &mut pending);
        }
    }

    discovery
        .files
        .sort_by(|left, right| left.path.cmp(&right.path));

    Ok(discovery)
}

impl Discovery {
    /// Records every entry of `directory` and queues its subdirectories.
    fn scan_directory(
        &mut self,
        directory: &Path,
        relative: &str,
        options: &AuditOptions,
        pending: &mut Vec<(PathBuf, String)>,
    ) {
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) => {
                self.files
                    .push(read_failure(&relative_or_dot(relative), &error));

                return;
            }
        };

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    self.files
                        .push(read_failure(&relative_or_dot(relative), &error));

                    continue;
                }
            };

            let name = entry.file_name().to_string_lossy().into_owned();
            let child = if relative.is_empty() {
                name.clone()
            } else {
                format!("{relative}/{name}")
            };

            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) => {
                    self.files.push(read_failure(&child, &error));

                    continue;
                }
            };

            if file_type.is_dir() {
                if SKIPPED_DIRECTORIES.contains(&name.as_str()) {
                    self.excluded_directories += 1;
                } else {
                    pending.push((entry.path(), child));
                }

                continue;
            }

            let info = match entry.metadata() {
                Ok(info) => info,
                Err(error) => {
                    self.files.push(read_failure(&child, &error));

                    continue;
                }
            };

            if file_type.is_symlink() {
                self.record_symlink(child, info.len());
            } else if file_type.is_file() {
                self.files
                    .push(visit_file(&entry.path(), child, info.len(), options));
            }
        }
    }

    fn record_symlink(&mut self, path: String, bytes: u64) {
        self.symlinks += 1;
        self.files.push(DiscoveredFile {
            category: category_for_path(&path),
            path,
            status: FileStatus::Excluded,
            reason: "symlink not followed".to_owned(),
            error: String::new(),
            language: None,
            bytes,
            source: None,
        });
    }
}

/// Classifies one regular file and reads it when it is supported source
/// within the size limit.
fn visit_file(path: &Path, relative: String, size: u64, options: &AuditOptions) -> DiscoveredFile {
    let category = category_for_path(&relative);
    let language = language_for_path(&relative);

    let mut file = DiscoveredFile {
        path: relative,
        category,
        status: FileStatus::Excluded,
        reason: String::new(),
        error: String::new(),
        language,
        bytes: size,
        source: None,
    };

    let Some(language) = language else {
        file.status = FileStatus::Unsupported;
        file.reason = "language is not supported".to_owned();

        return file;
    };

    if options
        .max_file_bytes
        .is_some_and(|limit| limit > 0 && size > limit)
    {
        file.reason = "file exceeds max_file_bytes".to_owned();

        return file;
    }

    let source = match fs::read(path) {
        Ok(source) => source,
        Err(error) => return read_failure(&file.path, &error),
    };

    if file.category != Category::Vendor && is_generated(language, &source) {
        file.category = Category::Generated;
    }

    file.bytes = source.len() as u64;
    file.source = Some(source);

    let excluded = match file.category {
        Category::Vendor if !options.include_vendor => "vendor excluded by configuration",
        Category::Generated if !options.include_generated => "generated excluded by configuration",
        Category::Test if !options.include_tests => "tests excluded by configuration",
        Category::Testdata if !options.include_testdata => "testdata excluded by configuration",
        _ => "",
    };

    if excluded.is_empty() {
        file.status = FileStatus::Analyzed;
    } else {
        file.reason = excluded.to_owned();
    }

    file
}

/// A file that could not be read or listed. Its category follows its name
/// when it looks like supported source, else it is `unsupported`.
fn read_failure(relative: &str, error: &io::Error) -> DiscoveredFile {
    let language = language_for_path(relative);
    let category = if language.is_some() {
        category_for_path(relative)
    } else {
        Category::Unsupported
    };

    DiscoveredFile {
        path: relative.to_owned(),
        category,
        status: FileStatus::ReadError,
        reason: "source could not be analyzed".to_owned(),
        error: io_message(error),
        language,
        bytes: 0,
        source: None,
    }
}

fn relative_or_dot(relative: &str) -> String {
    if relative.is_empty() {
        ".".to_owned()
    } else {
        relative.to_owned()
    }
}

/// The OS error text without Rust's ` (os error N)` suffix, lowercased like
/// Go's `syscall` messages (`permission denied`).
fn io_message(error: &io::Error) -> String {
    let text = error.to_string();
    let text = text.split(" (os error").next().unwrap_or(&text);
    let mut chars = text.chars();

    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// `.go` is Go; `.ts .mts .cts` (including `.d.ts`) is TypeScript; `.tsx` is TSX.
fn language_for_path(path: &str) -> Option<Language> {
    if path.ends_with(".go") {
        return Some(Language::Go);
    }

    let name = path.rsplit('/').next().unwrap_or(path);

    match name.rfind('.').map(|dot| &name[dot..]) {
        Some(".ts" | ".mts" | ".cts") => Some(Language::TypeScript),
        Some(".tsx") => Some(Language::Tsx),
        _ => None,
    }
}

/// Category from the path alone. The first directory component that is
/// `vendor`, `__tests__` (TypeScript only), or `testdata` decides; else the
/// file name decides.
fn category_for_path(path: &str) -> Category {
    let mut parts: Vec<&str> = path.split('/').collect();
    let name = parts.pop().unwrap_or_default();
    let typescript = matches!(
        language_for_path(path),
        Some(Language::TypeScript | Language::Tsx)
    );

    for part in parts {
        if part == "vendor" {
            return Category::Vendor;
        }

        if part == "__tests__" && typescript {
            return Category::Test;
        }

        if part == "testdata" {
            return Category::Testdata;
        }
    }

    if name.ends_with("_test.go") {
        return Category::Test;
    }

    if typescript {
        let stem = name.rfind('.').map_or(name, |dot| &name[..dot]);

        if stem.ends_with(".test") || stem.ends_with(".spec") {
            return Category::Test;
        }
    }

    Category::Production
}

/// Whether the file's header marks it as generated: a line that starts
/// `// Code generated ` and ends ` DO NOT EDIT.` in a comment before the Go
/// `package` clause, or among the leading `//` comment lines of TypeScript.
fn is_generated(language: Language, source: &[u8]) -> bool {
    let text = String::from_utf8_lossy(source);

    if language != Language::Go {
        for line in text.split('\n') {
            let line = line.trim();

            if line.is_empty() {
                continue;
            }

            if !line.starts_with("//") {
                return false;
            }

            if is_generated_marker(line) {
                return true;
            }
        }

        return false;
    }

    // Go: scan the comments before the package clause. Anything else before
    // `package` makes the header invalid, which `go/parser` reports as an error.
    let mut rest = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let mut generated = false;

    loop {
        rest = rest.trim_start();

        if let Some(after) = rest.strip_prefix("//") {
            let end = after.find('\n').unwrap_or(after.len());
            let comment = format!("//{}", &after[..end]);
            generated |= is_generated_marker(comment.trim_end_matches('\r'));
            rest = &after[end..];
        } else if let Some(after) = rest.strip_prefix("/*") {
            let Some(end) = after.find("*/") else {
                return false;
            };

            let comment = format!("/*{}*/", &after[..end]);
            generated |= comment
                .split('\n')
                .any(|line| is_generated_marker(line.trim_end_matches('\r')));
            rest = &after[end + 2..];
        } else {
            break;
        }
    }

    let starts_package = rest.strip_prefix("package").is_some_and(|after| {
        after
            .chars()
            .next()
            .is_some_and(|next| next.is_whitespace() || next == '/')
    });

    generated && starts_package
}

fn is_generated_marker(line: &str) -> bool {
    line.strip_prefix("// Code generated ")
        .is_some_and(|rest| rest.ends_with(" DO NOT EDIT."))
}
