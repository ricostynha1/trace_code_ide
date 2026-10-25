/// @implements REQ-DRT-SCHEMA.json_shape_matches
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
