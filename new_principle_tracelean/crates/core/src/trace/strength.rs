use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Strength {
    Pinned { theorem_name: String },
    /// @implements REQ-STRENGTH.attempted_distinguished
    Attempted { theorem_name: String },
    /// @implements REQ-STRENGTH.open_is_the_default
    Open,
    /// @implements REQ-STRENGTH.nondeterministic_declared
    Nondeterministic { reason: String },
}

impl Strength {
    pub fn is_settled(&self) -> bool {
        matches!(self, Strength::Pinned { .. } | Strength::Nondeterministic { .. })
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Strength::Pinned { .. } => "pinned",
            Strength::Attempted { .. } => "attempted",
            Strength::Open => "open",
            Strength::Nondeterministic { .. } => "nondeterministic",
        }
    }
}
