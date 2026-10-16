//! Stable keys for recall records, used to tell a sent record from a new one.

use sha2::{Digest, Sha256};

/// Hex characters kept from the SHA-256 digest; 64 bits is enough to key one session.
pub(crate) const HASH_HEX_CHARS: usize = 16;

/// A stable key for one recall record: whitespace layout does not change it.
pub(crate) fn record_hash(record: &str) -> String {
    let normalized = record.split_whitespace().collect::<Vec<_>>().join(" ");
    hash_prefix(normalized.as_bytes())
}

/// The first [`HASH_HEX_CHARS`] lowercase hex characters of the SHA-256 of `bytes`.
pub(crate) fn hash_prefix(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
        .chars()
        .take(HASH_HEX_CHARS)
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use proptest::prelude::*;

    use super::{HASH_HEX_CHARS, record_hash};

    #[test]
    fn a_key_has_the_fixed_length_of_hex_digits() {
        let key = record_hash("[t] one record");

        assert_eq!(key.len(), HASH_HEX_CHARS);
        assert!(key.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn an_empty_record_still_has_a_key() {
        assert_eq!(record_hash("").len(), HASH_HEX_CHARS);
    }

    proptest! {
        /// Whitespace layout never changes the key, so the same words always dedupe.
        #[test]
        fn whitespace_layout_never_changes_the_key(
            words in proptest::collection::vec("[a-z]{1,6}", 0..8),
            gap in "[ \t\n]{1,4}",
        ) {
            let single = words.join(" ");
            let spaced = words.join(&gap);

            prop_assert_eq!(record_hash(&spaced), record_hash(&single));
        }
    }
}
