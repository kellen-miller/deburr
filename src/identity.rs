//! Every digest and ID scheme lives here, so changing a scheme edits one file.

use sha2::{Digest, Sha256};

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

#[cfg(test)]
mod tests {
    use super::{ManifestEntry, manifest_digest, sha256_prefixed};

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
