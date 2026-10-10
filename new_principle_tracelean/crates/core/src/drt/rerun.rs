//! When a differential run is run again. Apart from `auto`, which reaches for
//! scratch directories, so that what is bound here reads only its arguments.

/// A differential run held for a clause: its op, whether it agreed at L3,
/// what keeps it valid, and the hashes its inputs have now.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HeldRun {
    pub op: String,
    pub agreed: bool,
    pub record: crate::trace::record::StalenessInput,
    pub current: Vec<(String, String)>,
}

/// Whether `op` is run again: when asked, or when no held run of it agreed
/// against inputs and a link that are still what they were — running it
/// again could only say the same.
///
/// @implements REQ-STALE.agreed_not_rerun
/// @drt REQ-STALE.agreed_not_rerun
pub fn rerun(held: Vec<HeldRun>, op: String, live: Vec<String>, again: bool) -> bool {
    again
        || !held.into_iter().any(|h| {
            h.op == op && h.agreed && crate::trace::record::staleness_of(h.record, live.clone(), h.current).is_none()
        })
}
