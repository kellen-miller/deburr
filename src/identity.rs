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

/// The text of `tokens`, each followed by a zero byte: the content that
/// finding IDs, fingerprints, and branch equality are computed from.
#[must_use]
pub fn token_content(syntax: &Syntax, tokens: &[Token]) -> Vec<u8> {
    let mut content = Vec::new();

    for token in tokens {
        content.extend_from_slice(syntax.token_text(token));
        content.push(0);
    }

    content
}

/// What a finding's identity is derived from.
#[derive(Debug, Clone, Copy)]
pub struct FindingSketch<'a> {
    pub path: &'a str,
    pub rule: &'a str,
    /// The identity name of the owning function (the part of its ID after `::`).
    pub owner_identity: &'a str,
    /// `sha256:<hex>` fingerprint of the owning function.
    pub owner_fingerprint: &'a str,
    /// [`token_content`] of the flagged code.
    pub content: &'a [u8],
    /// 1-based count of this rule's findings in the owner, in source order.
    pub occurrence: usize,
}

/// `<rule>-<16 hex>`: the finding ID, stable while the flagged code, its
/// owner's identity, and its position among the owner's findings hold.
#[must_use]
pub fn finding_id(sketch: &FindingSketch<'_>) -> String {
    let mut hasher = Sha256::new();

    for part in [
        sketch.path.as_bytes(),
        sketch.rule.as_bytes(),
        sketch.owner_identity.as_bytes(),
        sketch.content,
    ] {
        hasher.update(part);
        hasher.update([0]);
    }

    hasher.update(sketch.occurrence.to_string());

    format!("{}-{}", sketch.rule, hex::encode(&hasher.finalize()[..8]))
}

/// Digest of the flagged code and the function that holds it, so an edit to
/// either marks the finding as changed.
#[must_use]
pub fn finding_fingerprint(sketch: &FindingSketch<'_>) -> String {
    let mut hasher = Sha256::new();

    hasher.update(b"finding\0");
    hasher.update(sketch.owner_fingerprint.as_bytes());
    hasher.update([0]);
    hasher.update(sketch.content);

    hex::encode(hasher.finalize())
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
        FindingSketch, ManifestEntry, UnitSketch, assign_unit_identities, finding_fingerprint,
        finding_id, function_id, manifest_digest, sha256_prefixed,
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

    fn finding<'a>(
        owner_fingerprint: &'a str,
        content: &'a [u8],
        occurrence: usize,
    ) -> FindingSketch<'a> {
        FindingSketch {
            path: "a.go",
            rule: "duplicate-branches",
            owner_identity: "F",
            owner_fingerprint,
            content,
            occurrence,
        }
    }

    #[test]
    fn finding_ids_have_a_rule_prefix_and_sixteen_hex_digits() {
        let id = finding_id(&finding("sha256:aa", b"x\0", 1));
        let digits = id.strip_prefix("duplicate-branches-").expect("rule prefix");

        assert_eq!(digits.len(), 16);
        assert!(digits.chars().all(|digit| digit.is_ascii_hexdigit()));
    }

    #[test]
    fn finding_ids_ignore_the_owner_fingerprint_but_fingerprints_do_not() {
        let base = finding("sha256:aa", b"x\0", 1);
        let edited_owner = finding("sha256:bb", b"x\0", 1);
        let other_content = finding("sha256:aa", b"y\0", 1);
        let second = finding("sha256:aa", b"x\0", 2);

        assert_eq!(finding_id(&base), finding_id(&edited_owner));
        assert_ne!(
            finding_fingerprint(&base),
            finding_fingerprint(&edited_owner)
        );
        assert_ne!(finding_id(&base), finding_id(&other_content));
        assert_ne!(finding_id(&base), finding_id(&second));
        assert_eq!(finding_fingerprint(&base), finding_fingerprint(&second));
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
