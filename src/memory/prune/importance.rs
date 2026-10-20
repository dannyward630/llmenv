//! The importance levels that ICM reports for a memory record.

/// Parsed importance levels from the MCP output.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Importance {
    Critical,
    High,
    Medium,
    Low,
    /// A level that ICM may add later. Prune forgets for good, so a record it cannot place is
    /// kept.
    Unknown,
}

impl Importance {
    pub(super) fn from_str(s: &str) -> Self {
        match s.trim() {
            "critical" => Self::Critical,
            "high" => Self::High,
            "medium" => Self::Medium,
            "low" => Self::Low,
            _ => Self::Unknown,
        }
    }

    pub(super) fn should_prune(&self) -> bool {
        matches!(self, Self::Medium | Self::Low)
    }
}
