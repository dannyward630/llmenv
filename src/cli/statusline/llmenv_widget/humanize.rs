//! Byte counts for the status line, humanized to `MB`, `KB`, or `B`.

/// Humanize a byte count at the 1024 and 1024 squared boundaries.
pub(super) fn humanize_bytes(bytes: u64) -> String {
    const MB: u64 = 1024 * 1024;
    const KB: u64 = 1024;
    if bytes >= MB {
        format!("{} MB", bytes / MB)
    } else if bytes >= KB {
        format!("{} KB", bytes / KB)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Boundary behavior at 1024 / 1024^2 across the full `u64` space —
        /// previously only unit-tested at fixed values.
        #[test]
        fn humanize_bytes_respects_unit_thresholds(bytes in any::<u64>()) {
            const KB: u64 = 1024;
            const MB: u64 = 1024 * 1024;
            let out = humanize_bytes(bytes);
            if bytes >= MB {
                prop_assert_eq!(out, format!("{} MB", bytes / MB));
            } else if bytes >= KB {
                prop_assert_eq!(out, format!("{} KB", bytes / KB));
            } else {
                prop_assert_eq!(out, format!("{bytes} B"));
            }
        }
    }
}
