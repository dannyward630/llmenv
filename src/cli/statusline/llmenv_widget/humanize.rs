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

    #[test]
    fn humanize_bytes_switches_units_at_the_boundaries() {
        assert_eq!(humanize_bytes(0), "0 B");
        assert_eq!(humanize_bytes(1023), "1023 B");
        assert_eq!(humanize_bytes(1024), "1 KB");
        assert_eq!(humanize_bytes(1024 * 1024 - 1), "1023 KB");
        assert_eq!(humanize_bytes(1024 * 1024), "1 MB");
        assert_eq!(humanize_bytes(u64::MAX), "17592186044415 MB");
    }
}
