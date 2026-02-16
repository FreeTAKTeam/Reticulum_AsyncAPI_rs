use rmp_serde::{decode::Error as DecodeError, encode::Error as EncodeError};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CodecError {
    #[error("failed to serialize payload to JSON value: {0}")]
    JsonSerialize(#[source] serde_json::Error),
    #[error("failed to encode canonical messagepack: {0}")]
    MessagePackEncode(#[source] EncodeError),
    #[error("failed to decode messagepack payload: {0}")]
    MessagePackDecode(#[source] DecodeError),
    #[error("failed to deserialize decoded payload to target type: {0}")]
    JsonDeserialize(#[source] serde_json::Error),
}

pub fn encode_canonical<T: Serialize>(value: &T) -> Result<Vec<u8>, CodecError> {
    let json = serde_json::to_value(value).map_err(CodecError::JsonSerialize)?;
    let normalized = normalize_json(json);
    rmp_serde::to_vec_named(&normalized).map_err(CodecError::MessagePackEncode)
}

pub fn decode_canonical<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, CodecError> {
    let decoded: Value = rmp_serde::from_slice(bytes).map_err(CodecError::MessagePackDecode)?;
    serde_json::from_value(decoded).map_err(CodecError::JsonDeserialize)
}

fn normalize_json(value: Value) -> Value {
    match value {
        Value::Object(obj) => {
            let mut keys: Vec<String> = obj.keys().cloned().collect();
            keys.sort();

            let mut normalized = Map::new();
            for key in keys {
                if let Some(item) = obj.get(&key) {
                    normalized.insert(key, normalize_json(item.clone()));
                }
            }
            Value::Object(normalized)
        }
        Value::Array(items) => {
            let normalized = items.into_iter().map(normalize_json).collect();
            Value::Array(normalized)
        }
        primitive => primitive,
    }
}

#[cfg(test)]
mod tests {
    use super::{decode_canonical, encode_canonical};
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    struct Nested {
        alpha: String,
        beta: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    struct Sample {
        zulu: String,
        nested: Nested,
    }

    #[test]
    fn round_trip_is_stable() {
        let sample = Sample {
            zulu: "z".to_string(),
            nested: Nested {
                alpha: "a".to_string(),
                beta: "b".to_string(),
            },
        };

        let encoded = encode_canonical(&sample).expect("encode");
        let decoded: Sample = decode_canonical(&encoded).expect("decode");

        assert_eq!(sample, decoded);
    }
}
