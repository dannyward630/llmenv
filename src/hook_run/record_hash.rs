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
