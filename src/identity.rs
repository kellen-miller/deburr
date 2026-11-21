//! Every digest and ID scheme lives here, so changing a scheme edits one file.

use std::collections::HashMap;

use sha2::{Digest, Sha256};

use crate::syntax::{Syntax, Token};

/// `sha256:<hex>` of `data`.
#[must_use]
pub fn sha256_prefixed(data: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(data)))
}

/// One file's contribution to the source manifest.
#[derive(Debug, Clone, Copy)]
pub struct ManifestEntry<'a> {
    pub path: &'a str,
    /// The bytes that were read; empty for files that were not read.
    pub source: &'a [u8],
}

/// `sha256:<hex>` over every discovered file in path order. Each file is
/// framed as `<path length>:<path>:<source length>:<source>\0`, so renaming,
/// adding, removing, or editing any file changes the digest.
#[must_use]
pub fn manifest_digest(entries: &[ManifestEntry<'_>]) -> String {
    let mut ordered = entries.to_vec();
    ordered.sort_by(|left, right| left.path.cmp(right.path));

    let mut hasher = Sha256::new();

    for entry in ordered {
        hasher.update(format!(
            "{}:{}:{}:",
            entry.path.len(),
            entry.path,
            entry.source.len()
        ));
        hasher.update(entry.source);
        hasher.update([0]);
    }

    format!("sha256:{}", hex::encode(hasher.finalize()))
}

/// `sha256:<hex>` over the text of `tokens`, each followed by a zero byte.
/// This is the one token-text form every fingerprint and equality check
/// uses; synthetic statement terminators are tokens like any other.
#[must_use]
pub fn token_fingerprint(syntax: &Syntax, tokens: &[Token]) -> String {
    let mut hasher = Sha256::new();

    for token in tokens {
        hasher.update(syntax.token_text(token));
        hasher.update([0]);
    }

    format!("sha256:{}", hex::encode(hasher.finalize()))
}

/// `<package>/<path>::<identity>`: the report ID of a function.
#[must_use]
pub fn function_id(package: &str, path: &str, identity: &str) -> String {
    format!("{package}/{path}::{identity}")
}

/// What identity assignment needs to know about one function unit. Units are
/// listed in source pre-order, so a parent always precedes its children.
#[derive(Debug, Clone, Copy)]
pub struct UnitSketch<'a> {
    /// Display name (`Server.Start`, `file.func2`, `Outer.func1`).
    pub name: &'a str,
    /// Index of the innermost enclosing unit.
    pub parent: Option<usize>,
    /// A declared function rather than a literal.
    pub declaration: bool,
    /// `sha256:<hex>` over the unit's token text.
    pub fingerprint: &'a str,
}

/// The stable identity of one function unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitIdentity {
    /// The part of the report ID after `::`.
    pub name: String,
    /// The identity cannot tell this unit from a sibling.
    pub ambiguous: bool,
}

/// Assigns identities. Top-level units are named by their display name;
/// nested units are `<parent identity>.anon-<fingerprint digest>`, so a
/// closure keeps its identity when siblings are added or reordered. A repeat
/// of an identity under one parent gets `#2`, `#3`, ... in source order, and
/// every repeat and everything below a repeat is ambiguous. Literals that
/// share a parent and a fingerprint are ambiguous too.
#[must_use]
pub fn assign_unit_identities(units: &[UnitSketch<'_>]) -> Vec<UnitIdentity> {
    let mut top_level_names: HashMap<&str, usize> = HashMap::new();

    for unit in units.iter().filter(|unit| unit.parent.is_none()) {
        *top_level_names.entry(unit.name).or_default() += 1;
    }

    let mut occurrences: HashMap<(Option<usize>, String), usize> = HashMap::new();
    let mut identities: Vec<UnitIdentity> = Vec::with_capacity(units.len());

    for unit in units {
        let (base, mut ambiguous) = match unit.parent {
            None => (
                unit.name.to_owned(),
                top_level_names
                    .get(unit.name)
                    .is_some_and(|&count| count > 1),
            ),
            Some(parent) => {
                let digest = unit
                    .fingerprint
                    .strip_prefix("sha256:")
                    .unwrap_or(unit.fingerprint);

                (
                    format!("{}.anon-{digest}", identities[parent].name),
                    identities[parent].ambiguous,
                )
            }
        };

        let seen = occurrences.entry((unit.parent, base.clone())).or_default();
        let name = if *seen > 0 {
            ambiguous = true;
            format!("{base}#{}", *seen + 1)
        } else {
            base
        };
        *seen += 1;

        identities.push(UnitIdentity { name, ambiguous });
    }

    let mut siblings: HashMap<(Option<usize>, &str), Vec<usize>> = HashMap::new();

    for (index, unit) in units
        .iter()
        .enumerate()
        .filter(|(_, unit)| !unit.declaration)
    {
        siblings
            .entry((unit.parent, unit.fingerprint))
            .or_default()
            .push(index);
    }

    for group in siblings.values().filter(|group| group.len() > 1) {
        for &index in group {
            identities[index].ambiguous = true;
        }
    }

    identities
}

#[cfg(test)]
mod tests {
    use super::{
        ManifestEntry, UnitSketch, assign_unit_identities, function_id, manifest_digest,
        sha256_prefixed,
    };

    fn sketch<'a>(
        name: &'a str,
        parent: Option<usize>,
        declaration: bool,
        fingerprint: &'a str,
    ) -> UnitSketch<'a> {
        UnitSketch {
            name,
            parent,
            declaration,
            fingerprint,
        }
    }

    #[test]
    fn function_ids_join_package_path_and_identity() {
        assert_eq!(
            function_id("a", "a/a.go", "Server.Start"),
            "a/a/a.go::Server.Start"
        );
    }

    #[test]
    fn repeated_top_level_names_get_suffixes_and_ambiguity() {
        let units = [
            sketch("init", None, true, "sha256:aa"),
            sketch("init", None, true, "sha256:aa"),
            sketch("Run", None, true, "sha256:bb"),
        ];
        let identities = assign_unit_identities(&units);

        assert_eq!(identities[0].name, "init");
        assert_eq!(identities[1].name, "init#2");
        assert_eq!(identities[2].name, "Run");
        assert!(identities[0].ambiguous && identities[1].ambiguous);
        assert!(!identities[2].ambiguous);
    }

    #[test]
    fn nested_units_follow_content_and_inherit_ambiguity() {
        let units = [
            sketch("Outer", None, true, "sha256:aa"),
            sketch("Outer.func1", Some(0), false, "sha256:11"),
            sketch("Outer.func2", Some(0), false, "sha256:22"),
            sketch("init", None, true, "sha256:cc"),
            sketch("init", None, true, "sha256:cc"),
            sketch("init.func1", Some(4), false, "sha256:11"),
        ];
        let identities = assign_unit_identities(&units);

        assert_eq!(identities[1].name, "Outer.anon-11");
        assert_eq!(identities[2].name, "Outer.anon-22");
        assert!(!identities[1].ambiguous && !identities[2].ambiguous);
        assert_eq!(identities[5].name, "init#2.anon-11");
        assert!(identities[5].ambiguous);
    }

    #[test]
    fn literals_with_equal_content_under_one_parent_are_ambiguous() {
        let units = [
            sketch("Outer", None, true, "sha256:aa"),
            sketch("Outer.func1", Some(0), false, "sha256:11"),
            sketch("Outer.func2", Some(0), false, "sha256:11"),
        ];
        let identities = assign_unit_identities(&units);

        assert_eq!(identities[1].name, "Outer.anon-11");
        assert_eq!(identities[2].name, "Outer.anon-11#2");
        assert!(identities[1].ambiguous && identities[2].ambiguous);
        assert!(!identities[0].ambiguous);
    }

    #[test]
    fn digest_of_empty_input() {
        assert_eq!(
            sha256_prefixed(b""),
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn manifest_digest_ignores_input_order_and_sees_content() {
        let a = ManifestEntry {
            path: "a.go",
            source: b"package a\n",
        };
        let b = ManifestEntry {
            path: "b.go",
            source: b"",
        };
        let edited = ManifestEntry {
            path: "a.go",
            source: b"package b\n",
        };

        assert_eq!(manifest_digest(&[a, b]), manifest_digest(&[b, a]));
        assert_ne!(manifest_digest(&[a, b]), manifest_digest(&[edited, b]));
    }
}
