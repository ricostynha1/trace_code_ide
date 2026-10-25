#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HeldRun {
    pub op: String,
    pub agreed: bool,
    pub record: crate::trace::record::StalenessInput,
    pub current: Vec<(String, String)>,
}

/// @implements REQ-STALE.agreed_not_rerun
/// @drt REQ-STALE.agreed_not_rerun
pub fn rerun(held: Vec<HeldRun>, op: String, live: Vec<String>, again: bool) -> bool {
    again
        || !held.into_iter().any(|h| {
            h.op == op && h.agreed && crate::trace::record::staleness_of(h.record, live.clone(), h.current).is_none()
        })
}
