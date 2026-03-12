//! Canonical serialisation for object signing.
//!
//! Rule: sort JSON object keys lexicographically, compact (no extra whitespace).
//! Content IDs use BLAKE3, matching Veilid's VLD0 crypto suite.

use crate::CoreError;
use serde::Serialize;

/// Serialise `value` into canonical JSON bytes (keys sorted, compact).
pub fn canonical_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, CoreError> {
    let v = serde_json::to_value(value).map_err(|e| CoreError::Canon(e.to_string()))?;
    let ordered = sort_value(v);
    serde_json::to_vec(&ordered).map_err(|e| CoreError::Canon(e.to_string()))
}

/// BLAKE3 hash of canonical bytes → hex string used as object_id.
pub fn content_id<T: Serialize>(value: &T) -> Result<String, CoreError> {
    let bytes = canonical_bytes(value)?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

fn sort_value(v: serde_json::Value) -> serde_json::Value {
    match v {
        serde_json::Value::Object(map) => {
            let mut sorted = serde_json::Map::new();
            let mut keys: Vec<std::string::String> = map.keys().cloned().collect();
            keys.sort();
            for k in keys {
                sorted.insert(k.clone(), sort_value(map[&k].clone()));
            }
            serde_json::Value::Object(sorted)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(sort_value).collect())
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn stable_across_key_order() {
        let a = json!({"z": 1, "a": 2});
        let b = json!({"a": 2, "z": 1});
        assert_eq!(
            serde_json::to_vec(&sort_value(a)).unwrap(),
            serde_json::to_vec(&sort_value(b)).unwrap()
        );
    }

    #[test]
    fn content_id_is_deterministic() {
        #[derive(Serialize)]
        struct Foo {
            z: u32,
            a: u32,
        }
        let id1 = content_id(&Foo { z: 1, a: 2 }).unwrap();
        let id2 = content_id(&Foo { z: 1, a: 2 }).unwrap();
        assert_eq!(id1, id2);
        assert_eq!(id1.len(), 64); // hex BLAKE3
    }
}
