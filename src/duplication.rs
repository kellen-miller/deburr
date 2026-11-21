//! Built-in clone detection: the only module that touches `cpd_core`.
//!
//! deburr hands `cpd_core` the code tokens of one category's analyzed files
//! and gets back pairs of equal token runs. Everything a reader sees about a
//! clone (code-line counts, the functions it overlaps, IDs, percentages) is
//! computed here from deburr's own line map and function table, never from
//! the detector's line arithmetic.

use std::collections::HashMap;

use cpd_core::detect::{PathFilters, PreparedSource, detect_prepared};
use cpd_core::hash::token_hash;
use cpd_core::models::{DetectionToken, Fragment, Location};

use crate::analysis::FunctionBody;
use crate::identity::{self, CloneEndpointSketch};
use crate::report::{Category, CloneEndpoint, CodeClone, DuplicationCategory, Function, Position};
use crate::syntax::{Language, Token};

/// Detection settings exposed as `--min-clone-tokens` and `--min-clone-lines`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DuplicationSettings {
    pub min_tokens: usize,
    pub min_lines: usize,
}

/// One analyzed file of the category being measured, borrowed for detection.
#[derive(Debug, Clone, Copy)]
pub struct AnalyzedFile<'a> {
    pub path: &'a str,
    pub language: Language,
    pub source: &'a [u8],
    pub tokens: &'a [Token],
    /// Index `n - 1` is true when line `n` holds code.
    pub code_lines: &'a [bool],
    /// The file's functions as reported, so clone endpoints can name them.
    pub functions: &'a [Function],
    /// Each function's body bytes, keyed by function ID.
    pub bodies: &'a [FunctionBody],
}

/// The measurement and clones for one category.
#[derive(Debug)]
pub struct CategoryDuplication {
    pub measurement: DuplicationCategory,
    pub clones: Vec<CodeClone>,
}

#[derive(Debug, thiserror::Error)]
pub enum DuplicationError {
    #[error("build clone detection thread pool: {0}")]
    Pool(#[from] rayon::ThreadPoolBuildError),
}

/// One side of a clone that passed the line minimum.
struct Region<'a> {
    file: usize,
    endpoint: CloneEndpoint,
    /// The innermost function that holds the whole region.
    scope: Option<&'a Function>,
    /// [`identity::region_content`] of the region's tokens.
    content: Vec<u8>,
}

/// Format names group files so a Go file never pairs with a TypeScript one;
/// `.ts` and `.tsx` share a token model and a group.
const FORMATS: [&str; 2] = ["go", "typescript"];

/// Finds clones among the files of one category.
///
/// The detector runs with a line minimum of zero. A clone is kept only when
/// both endpoints cover at least `settings.min_lines` code lines, so blank
/// and comment lines never help a clone reach the minimum. The percentage is
/// the distinct code lines covered by any kept endpoint over the category's
/// code lines, so it never exceeds 100.
///
/// # Errors
///
/// Returns an error when the detection thread pool cannot be built.
pub fn detect(
    files: &[AnalyzedFile<'_>],
    category: Category,
    settings: &DuplicationSettings,
) -> Result<CategoryDuplication, DuplicationError> {
    let mut groups: [Vec<PreparedSource>; 2] = [Vec::new(), Vec::new()];

    for file in files
        .iter()
        .filter(|file| file.tokens.len() >= settings.min_tokens)
    {
        let tokens: Vec<DetectionToken> = file
            .tokens
            .iter()
            .map(|token| {
                let text = String::from_utf8_lossy(identity::token_text(file.source, token));
                let hash = token_hash(0, &text);

                DetectionToken {
                    hash,
                    raw_hash: hash,
                    name: 0,
                    start: location(token.start, token.bytes.start),
                    end: location(token.end, token.bytes.end),
                    range: [token.bytes.start, token.bytes.end],
                }
            })
            .collect();
        let slot = match file.language {
            Language::Go => 0,
            Language::TypeScript | Language::Tsx => 1,
        };

        groups[slot].push(PreparedSource::from_detection_tokens(
            file.path.to_owned(),
            FORMATS[slot].to_owned(),
            &tokens,
        ));
    }

    let groups: Vec<Vec<PreparedSource>> = groups
        .into_iter()
        .filter(|group| !group.is_empty())
        .collect();
    let pool = rayon::ThreadPoolBuilder::new().build()?;
    let detected =
        pool.install(|| detect_prepared(groups, settings.min_tokens, 0, &PathFilters::default()));

    let indexes: HashMap<&str, usize> = files
        .iter()
        .enumerate()
        .map(|(index, file)| (file.path, index))
        .collect();
    let mut covered: Vec<Vec<bool>> = files
        .iter()
        .map(|file| vec![false; file.code_lines.len()])
        .collect();
    let mut pairs: Vec<([Region<'_>; 2], u32)> = Vec::new();

    for clone in &detected {
        let regions = [&clone.fragment_a, &clone.fragment_b].map(|fragment| {
            let file = *indexes.get(fragment.source_id.as_str())?;

            region(file, &files[file], fragment)
        });
        let [Some(first), Some(second)] = regions else {
            continue;
        };

        if first.endpoint.code_lines.min(second.endpoint.code_lines)
            < u32::try_from(settings.min_lines).unwrap_or(u32::MAX)
        {
            continue;
        }

        for side in [&first, &second] {
            let lines = &mut covered[side.file];

            for line in side.endpoint.start.line..=side.endpoint.end.line {
                if files[side.file].code_lines.get(line as usize - 1) == Some(&true) {
                    lines[line as usize - 1] = true;
                }
            }
        }

        pairs.push(([first, second], clone.token_count));
    }

    let sketches: Vec<[CloneEndpointSketch<'_>; 2]> = pairs
        .iter()
        .map(|(regions, _)| {
            regions.each_ref().map(|region| CloneEndpointSketch {
                path: &region.endpoint.path,
                scope: region.scope.map_or("file", |function| function.id.as_str()),
                scope_ambiguous: region
                    .scope
                    .is_some_and(|function| function.identity_ambiguous),
                content: &region.content,
                start: region.endpoint.start,
                end: region.endpoint.end,
            })
        })
        .collect();
    let identities = identity::assign_clone_identities(category, &sketches);

    let mut clones: Vec<CodeClone> = pairs
        .into_iter()
        .zip(identities)
        .map(|((regions, tokens), identity)| {
            let [left, right] = regions.map(|region| region.endpoint);
            let endpoints = if identity.first == 0 {
                [left, right]
            } else {
                [right, left]
            };

            CodeClone {
                id: identity.id,
                family_id: identity.family_id,
                fingerprint: identity.fingerprint,
                category,
                tokens,
                identity_ambiguous: identity.ambiguous,
                endpoints,
            }
        })
        .collect();
    clones.sort_by(|left, right| left.id.cmp(&right.id));

    let code_lines = files
        .iter()
        .map(|file| file.code_lines.iter().filter(|&&code| code).count())
        .sum::<usize>();
    let duplicated = covered.iter().flatten().filter(|&&line| line).count();

    Ok(CategoryDuplication {
        measurement: DuplicationCategory {
            category,
            files: u32::try_from(files.len()).unwrap_or(u32::MAX),
            code_lines: u32::try_from(code_lines).unwrap_or(u32::MAX),
            duplicated_code_lines: u32::try_from(duplicated).unwrap_or(u32::MAX),
            percentage: if code_lines == 0 {
                0.0
            } else {
                duplicated as f64 / code_lines as f64 * 100.0
            },
        },
        clones,
    })
}

/// A token's place in the detector's terms: 1-based line, 0-based column.
fn location(position: Position, offset: usize) -> Location {
    Location::new(
        position.line,
        position.column.saturating_sub(1),
        u32::try_from(offset).unwrap_or(u32::MAX),
    )
}

/// Turns one detected fragment (token indexes into its file's tokens) into a
/// region. Synthetic terminators at either end are not part of the reported
/// span; a fragment of nothing but terminators has no region.
fn region<'a>(file: usize, analyzed: &AnalyzedFile<'a>, fragment: &Fragment) -> Option<Region<'a>> {
    let run = analyzed
        .tokens
        .get(fragment.range[0] as usize..=fragment.range[1] as usize)?;
    let first = run.iter().position(|token| !token.is_terminator())?;
    let last = run.iter().rposition(|token| !token.is_terminator())?;
    let run = &run[first..=last];
    let (start, end) = (run[0].start, run[run.len() - 1].end);

    // A function owns the region when a region token lies inside its body
    // (innermost body wins). Signature tokens, or the `func` keyword that
    // starts the next declaration, are outside every body they precede, so
    // they never make an owner; a one-line function whose body the clone
    // only partly covers still does.
    let (first_byte, last_byte) = (run[0].bytes.start, run[run.len() - 1].bytes.end);
    let bodies: Vec<(&Function, &FunctionBody)> = analyzed
        .bodies
        .iter()
        .filter(|body| body.bytes.start < last_byte && first_byte < body.bytes.end)
        .filter_map(|body| {
            analyzed
                .functions
                .iter()
                .find(|function| function.id == body.function_id)
                .map(|function| (function, body))
        })
        .collect();
    let innermost = |byte: usize| {
        bodies
            .iter()
            .filter(|(_, body)| body.bytes.contains(&byte))
            .min_by_key(|(_, body)| body.bytes.len())
            .copied()
    };

    let mut owners: Vec<(&Function, &FunctionBody)> = Vec::new();

    for token in run.iter().filter(|token| !token.is_terminator()) {
        if let Some(owner) = innermost(token.bytes.start)
            && !owners.iter().any(|(known, _)| known.id == owner.0.id)
        {
            owners.push(owner);
        }
    }

    let owners: Vec<&Function> = owners.into_iter().map(|(function, _)| function).collect();
    // The scope is the innermost owner whose declaration holds the whole
    // region (a clone of a whole function starts at its keyword, outside the
    // body), so it is always a listed function or the file.
    let scope = owners
        .iter()
        .copied()
        .filter(|function| function.start <= start && end <= function.end)
        .max_by_key(|function| (function.start, std::cmp::Reverse(function.end)));
    let functions: Vec<String> = owners.iter().map(|owner| owner.id.clone()).collect();
    let code_lines = (start.line..=end.line)
        .filter(|&line| analyzed.code_lines.get(line as usize - 1) == Some(&true))
        .count();

    Some(Region {
        file,
        endpoint: CloneEndpoint {
            path: analyzed.path.to_owned(),
            start,
            end,
            code_lines: u32::try_from(code_lines).unwrap_or(u32::MAX),
            functions,
        },
        scope,
        content: identity::region_content(analyzed.source, run),
    })
}
