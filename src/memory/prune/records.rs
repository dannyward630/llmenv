//! The records of the non-compact `icm_memory_recall` output that prune reads.

/// A parsed memory record from the non-compact `icm_memory_recall` output.
#[derive(Debug, Clone)]
pub(super) struct MemoryRecord {
    pub(super) id: String,
    pub(super) importance: Importance,
}

/// Parsed importance levels from the MCP output.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Importance {
    Critical,
    High,
    Medium,
    Low,
}

impl Importance {
    fn from_str(s: &str) -> Self {
        match s.trim() {
            "critical" => Self::Critical,
            "high" => Self::High,
            "medium" => Self::Medium,
            _ => Self::Low,
        }
    }

    pub(super) fn should_prune(&self) -> bool {
        matches!(self, Self::Medium | Self::Low)
    }
}

/// Parse the non-compact `icm_memory_recall` output into structured records.
///
/// The format (per ICM's `format_memory_output` with compact=false):
///
/// ```text
/// --- <id> ---
///   topic: ...
///   importance: ...
///   weight: ...
///   summary: ...
///   keywords: ...
///   raw: ...
///   score: ...
/// ```
pub(super) fn parse_recall_output(text: &str) -> Vec<MemoryRecord> {
    let mut records = Vec::new();
    let mut current_id: Option<String> = None;
    let mut current_importance: Option<Importance> = None;
    // `has_weight` guards against partial records: we only finalize a record
    // once both importance and weight have been seen.
    let mut has_weight = false;
    let mut weight_seen = false;

    // A record header starts at column 0, and the fields of a record are indented. Memory
    // content is free text, so a line inside it must not forge a header: only an unindented
    // header counts. The first `importance` and `weight` of a record win, so a later line in
    // the summary cannot change them.
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = line.strip_prefix("--- ") {
            // new record start: "--- <id> ---"
            if let Some(id) = rest.strip_suffix(" ---") {
                // Finalize previous record
                if let (Some(id), Some(importance)) = (current_id.take(), current_importance.take())
                    && has_weight
                {
                    records.push(MemoryRecord { id, importance });
                }
                has_weight = false;
                weight_seen = false;
                current_id = Some(id.to_string());
            }
        } else if let Some(rest) = trimmed.strip_prefix("importance:") {
            if current_importance.is_none() {
                current_importance = Some(Importance::from_str(rest));
            }
        } else if let Some(rest) = trimmed.strip_prefix("weight:")
            && !weight_seen
        {
            weight_seen = true;
            // `f64::from_str` also accepts `nan` and `inf`, which are not weights.
            has_weight = rest.trim().parse::<f64>().is_ok_and(f64::is_finite);
        }
    }

    // Finalize the last record
    if let (Some(id), Some(importance)) = (current_id, current_importance)
        && has_weight
    {
        records.push(MemoryRecord { id, importance });
    }

    records
}
