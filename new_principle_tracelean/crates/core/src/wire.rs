//! Wire shapes shared by anything that crosses the conformance protocol.
//!
//! The models derive their encodings, so the encoding is a consequence of the
//! datatype rather than a second description of it. That leaves the
//! implementation to meet them: Lean has no `BTreeMap`, and its association
//! lists encode as arrays of pairs. A Rust map that serialised as a JSON object
//! would disagree with its model about a value both sides compute identically.

/// A `BTreeMap` on the wire as `[[key, value], …]`, sorted by key.
pub mod pairs {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::BTreeMap;

    pub fn serialize<S: Serializer>(
        map: &BTreeMap<String, String>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let items: Vec<(&String, &String)> = map.iter().collect();
        items.serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<BTreeMap<String, String>, D::Error> {
        let items: Vec<(String, String)> = Vec::deserialize(deserializer)?;
        Ok(items.into_iter().collect())
    }
}
