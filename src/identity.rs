//! Every digest and ID scheme lives here, so changing a scheme edits one file.

use sha2::{Digest, Sha256};

/// `sha256:<hex>` of `data`.
#[must_use]
pub fn sha256_prefixed(data: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(data)))
}

#[cfg(test)]
mod tests {
    use super::sha256_prefixed;

    #[test]
    fn digest_of_empty_input() {
        assert_eq!(
            sha256_prefixed(b""),
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
