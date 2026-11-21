//! Every digest and ID scheme lives here, so changing a scheme edits one file.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use sha2::{Digest, Sha256};

use crate::report::{Category, Position};
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

/// The text of one token: its source bytes, or `;` for a synthetic
/// statement terminator.
#[must_use]
pub fn token_text<'a>(source: &'a [u8], token: &Token) -> &'a [u8] {
    if token.is_terminator() {
        b";"
    } else {
        &source[token.bytes.clone()]
    }
}

/// The text of `tokens`, each followed by a zero byte: the content that
/// finding IDs, fingerprints, branch equality, and clone identity are
/// computed from.
#[must_use]
pub fn token_content(syntax: &Syntax, tokens: &[Token]) -> Vec<u8> {
    region_content(&syntax.source, tokens)
}

/// [`token_content`] for tokens of a file whose [`Syntax`] is no longer held.
#[must_use]
pub fn region_content(source: &[u8], tokens: &[Token]) -> Vec<u8> {
    let mut content = Vec::new();

    for token in tokens {
        content.extend_from_slice(token_text(source, token));
        content.push(0);
    }

    content
}

/// One side of a clone pair, as identity sees it.
#[derive(Debug, Clone, Copy)]
pub struct CloneEndpointSketch<'a> {
    pub path: &'a str,
    /// The ID of the innermost function that holds the whole region, or
    /// `file` when none does.
    pub scope: &'a str,
    pub scope_ambiguous: bool,
    /// [`region_content`] of the region.
    pub content: &'a [u8],
    pub start: Position,
    pub end: Position,
}

/// The identity of one clone pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloneIdentity {
    pub id: String,
    pub family_id: String,
    pub fingerprint: String,
    /// The pair cannot be told from another pair of equal content.
    pub ambiguous: bool,
    /// Index (0 or 1) of the endpoint that is listed first.
    pub first: usize,
}

/// An endpoint's place in the sorted set of every endpoint in a category.
/// Field order is the sort order, which fixes occurrence numbers.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct EndpointKey<'a> {
    digest: String,
    path: &'a str,
    start: Position,
    end: Position,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EndpointIdentity {
    /// `<content digest>#<occurrence>`.
    name: String,
    ambiguous: bool,
}

/// Assigns identities to the clone pairs of one category.
///
/// An endpoint is identified by what it contains and where it lives (path
/// and scope), not by its line numbers, so a clone keeps its ID when code
/// above it moves. Endpoints of equal content are numbered in position order
/// and are ambiguous, since only their order tells them apart. A family is
/// every pair connected through shared endpoints; its ID follows its member
/// endpoints.
#[must_use]
pub fn assign_clone_identities(
    category: Category,
    pairs: &[[CloneEndpointSketch<'_>; 2]],
) -> Vec<CloneIdentity> {
    let category = category.as_str().as_bytes();
    let keys: Vec<[EndpointKey<'_>; 2]> = pairs
        .iter()
        .map(|pair| {
            pair.map(|sketch| EndpointKey {
                digest: joined_digest(&[
                    b"clone-endpoint",
                    sketch.path.as_bytes(),
                    sketch.scope.as_bytes(),
                    sketch.content,
                ]),
                path: sketch.path,
                start: sketch.start,
                end: sketch.end,
            })
        })
        .collect();

    let mut scoped: BTreeMap<&EndpointKey<'_>, bool> = BTreeMap::new();

    for (pair, pair_keys) in pairs.iter().zip(&keys) {
        for (sketch, key) in pair.iter().zip(pair_keys) {
            scoped.insert(key, sketch.scope_ambiguous);
        }
    }

    let mut per_digest: HashMap<&str, usize> = HashMap::new();

    for key in scoped.keys() {
        *per_digest.entry(&key.digest).or_default() += 1;
    }

    let mut occurrences: HashMap<&str, usize> = HashMap::new();
    let mut endpoints: BTreeMap<&EndpointKey<'_>, EndpointIdentity> = BTreeMap::new();

    for (&key, &scope_ambiguous) in &scoped {
        let occurrence = occurrences.entry(&key.digest).or_default();
        *occurrence += 1;

        endpoints.insert(
            key,
            EndpointIdentity {
                name: format!("{}#{}", key.digest, *occurrence),
                ambiguous: scope_ambiguous
                    || per_digest
                        .get(key.digest.as_str())
                        .is_some_and(|&count| count > 1),
            },
        );
    }

    // A family is a connected set of endpoints; union-find over the sorted
    // endpoint order.
    let slots: BTreeMap<&EndpointKey<'_>, usize> = endpoints
        .keys()
        .enumerate()
        .map(|(slot, &key)| (key, slot))
        .collect();
    let mut parent: Vec<usize> = (0..slots.len()).collect();

    for pair_keys in &keys {
        let left = family_root(&mut parent, slots[&pair_keys[0]]);
        let right = family_root(&mut parent, slots[&pair_keys[1]]);

        parent[left] = right;
    }

    let mut members: BTreeMap<usize, BTreeSet<&str>> = BTreeMap::new();

    for (&key, endpoint) in &endpoints {
        members
            .entry(family_root(&mut parent, slots[key]))
            .or_default()
            .insert(&endpoint.name);
    }

    let family_ids: HashMap<usize, String> = members
        .iter()
        .map(|(&family, names)| {
            let mut parts: Vec<&[u8]> = vec![b"clone-family", category];
            parts.extend(names.iter().map(|name| name.as_bytes()));

            (family, format!("family-{}", &joined_digest(&parts)[..16]))
        })
        .collect();

    keys.iter()
        .map(|pair_keys| {
            let sides = pair_keys.each_ref().map(|key| &endpoints[key]);
            let first = usize::from(sides[0].name > sides[1].name);
            let mut digests = pair_keys.each_ref().map(|key| key.digest.as_bytes());

            digests.sort_unstable();

            let id = joined_digest(&[
                b"clone",
                category,
                sides[first].name.as_bytes(),
                sides[1 - first].name.as_bytes(),
            ]);

            CloneIdentity {
                id: format!("clone-{}", &id[..16]),
                family_id: family_ids[&family_root(&mut parent, slots[&pair_keys[0]])].clone(),
                fingerprint: joined_digest(&[b"clone-pair", category, digests[0], digests[1]]),
                ambiguous: sides.iter().any(|side| side.ambiguous),
                first,
            }
        })
        .collect()
}

/// The representative of `index`'s set, flattening the path as it goes.
fn family_root(parent: &mut [usize], mut index: usize) -> usize {
    while parent[index] != index {
        parent[index] = parent[parent[index]];
        index = parent[index];
    }

    index
}

/// Hex SHA-256 of `parts` separated by zero bytes.
fn joined_digest(parts: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();

    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            hasher.update([0]);
        }

        hasher.update(part);
    }

    hex::encode(hasher.finalize())
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

/// Assigns identities for languages that name units lexically: the name is
/// the identity, and a repeat of a name gets `#2`, `#3`, ... in source order.
/// A unit is ambiguous when another unit in the file has its name or when its
/// parent is ambiguous, so everything under a repeated function is too.
#[must_use]
pub fn assign_lexical_identities(units: &[UnitSketch<'_>]) -> Vec<UnitIdentity> {
    let mut totals: HashMap<&str, usize> = HashMap::new();

    for unit in units {
        *totals.entry(unit.name).or_default() += 1;
    }

    let mut seen: HashMap<&str, usize> = HashMap::new();
    let mut identities: Vec<UnitIdentity> = Vec::with_capacity(units.len());

    for unit in units {
        let occurrence = seen.entry(unit.name).or_default();
        *occurrence += 1;

        let shared = totals.get(unit.name).is_some_and(|&count| count > 1);
        let ambiguous = shared
            || unit
                .parent
                .is_some_and(|parent| identities[parent].ambiguous);
        let name = if *occurrence > 1 {
            format!("{}#{}", unit.name, *occurrence)
        } else {
            unit.name.to_owned()
        };

        identities.push(UnitIdentity { name, ambiguous });
    }

    identities
}

#[cfg(test)]
mod tests {
    use super::{
        CloneEndpointSketch, FindingSketch, ManifestEntry, UnitSketch, assign_clone_identities,
        assign_lexical_identities, assign_unit_identities, finding_fingerprint, finding_id,
        function_id, manifest_digest, sha256_prefixed,
    };
    use crate::report::{Category, Position};

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
    fn lexical_identities_are_names_with_suffixes_for_repeats() {
        let units = [
            sketch("run", None, true, "sha256:aa"),
            sketch("run.inner", Some(0), false, "sha256:11"),
            sketch("run", None, true, "sha256:bb"),
            sketch("run.inner", Some(2), false, "sha256:22"),
            sketch("solo", None, true, "sha256:cc"),
            sketch("solo.", Some(4), false, "sha256:33"),
        ];
        let identities = assign_lexical_identities(&units);
        let names: Vec<&str> = identities.iter().map(|id| id.name.as_str()).collect();

        assert_eq!(
            names,
            ["run", "run.inner", "run#2", "run.inner#2", "solo", "solo."]
        );
        // A repeated name is ambiguous for every holder, and so is anything
        // under one; a unique name under a unique parent is not.
        assert!(identities[..4].iter().all(|id| id.ambiguous));
        assert!(!identities[4].ambiguous && !identities[5].ambiguous);
    }

    #[test]
    fn a_unique_child_of_an_ambiguous_parent_is_ambiguous() {
        let units = [
            sketch("run", None, true, "sha256:aa"),
            sketch("run", None, true, "sha256:bb"),
            sketch("run.only", Some(1), false, "sha256:11"),
        ];
        let identities = assign_lexical_identities(&units);

        assert_eq!(identities[2].name, "run.only");
        assert!(identities[2].ambiguous);
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

    fn endpoint<'a>(path: &'a str, content: &'a [u8], line: u32) -> CloneEndpointSketch<'a> {
        CloneEndpointSketch {
            path,
            scope: "file",
            scope_ambiguous: false,
            content,
            start: Position { line, column: 1 },
            end: Position {
                line: line + 9,
                column: 1,
            },
        }
    }

    #[test]
    fn clone_ids_follow_content_and_not_line_numbers() {
        let here = assign_clone_identities(
            Category::Production,
            &[[endpoint("a.go", b"x\0", 3), endpoint("b.go", b"x\0", 5)]],
        );
        let moved = assign_clone_identities(
            Category::Production,
            &[[endpoint("a.go", b"x\0", 30), endpoint("b.go", b"x\0", 50)]],
        );
        let edited = assign_clone_identities(
            Category::Production,
            &[[endpoint("a.go", b"x\0", 3), endpoint("b.go", b"y\0", 5)]],
        );
        let tests = assign_clone_identities(
            Category::Test,
            &[[endpoint("a.go", b"x\0", 3), endpoint("b.go", b"x\0", 5)]],
        );

        assert_eq!(here, moved);
        assert!(!here[0].ambiguous);
        assert!(here[0].id.starts_with("clone-") && here[0].family_id.starts_with("family-"));
        assert_ne!(here[0].id, edited[0].id);
        assert_ne!(here[0].fingerprint, edited[0].fingerprint);
        assert_ne!(here[0].id, tests[0].id);
        assert_ne!(here[0].family_id, tests[0].family_id);
    }

    #[test]
    fn the_first_endpoint_does_not_depend_on_pair_order() {
        let forward = assign_clone_identities(
            Category::Production,
            &[[endpoint("a.go", b"x\0", 3), endpoint("b.go", b"x\0", 5)]],
        );
        let backward = assign_clone_identities(
            Category::Production,
            &[[endpoint("b.go", b"x\0", 5), endpoint("a.go", b"x\0", 3)]],
        );

        assert_eq!(forward[0].id, backward[0].id);
        assert_eq!(forward[0].fingerprint, backward[0].fingerprint);
        assert_ne!(forward[0].first, backward[0].first);
    }

    #[test]
    fn copies_of_one_region_in_one_place_are_ambiguous_and_share_a_family() {
        let pairs = [
            [endpoint("f.go", b"x\0", 1), endpoint("f.go", b"x\0", 20)],
            [endpoint("f.go", b"x\0", 1), endpoint("f.go", b"x\0", 40)],
            [endpoint("g.go", b"y\0", 1), endpoint("h.go", b"y\0", 1)],
        ];
        let identities = assign_clone_identities(Category::Production, &pairs);

        assert!(identities[0].ambiguous && identities[1].ambiguous);
        assert!(!identities[2].ambiguous);
        assert_eq!(identities[0].family_id, identities[1].family_id);
        assert_ne!(identities[0].family_id, identities[2].family_id);
        assert_ne!(identities[0].id, identities[1].id);
    }

    #[test]
    fn an_ambiguous_scope_makes_the_clone_ambiguous() {
        let mut left = endpoint("a.go", b"x\0", 3);
        left.scope = "pkg/a.go::init";
        left.scope_ambiguous = true;

        let identities =
            assign_clone_identities(Category::Production, &[[left, endpoint("b.go", b"x\0", 5)]]);

        assert!(identities[0].ambiguous);
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
