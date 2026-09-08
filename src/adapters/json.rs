//! Reject duplicate object keys before parsing provider JSON into a Value.
//! The validation pass discards scalar values; the normal pass retains JSON number precision.
use std::collections::BTreeSet;
use std::fmt;

use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};

use crate::ir::IrError;

struct Unique;
impl<'de> Deserialize<'de> for Unique {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Check;
        impl<'de> Visitor<'de> for Check {
            type Value = Unique;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("JSON with unique object keys")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Unique, A::Error> {
                let mut keys = BTreeSet::new();
                while let Some(key) = map.next_key::<String>()? {
                    if !keys.insert(key) {
                        return Err(serde::de::Error::custom("duplicate object key"));
                    }
                    map.next_value::<Unique>()?;
                }
                Ok(Unique)
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Unique, A::Error> {
                while seq.next_element::<Unique>()?.is_some() {}
                Ok(Unique)
            }
            fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<Unique, E> {
                Ok(Unique)
            }
            fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<Unique, E> {
                Ok(Unique)
            }
            fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<Unique, E> {
                Ok(Unique)
            }
            fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<Unique, E> {
                Ok(Unique)
            }
            fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<Unique, E> {
                Ok(Unique)
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Unique, E> {
                Ok(Unique)
            }
        }
        deserializer.deserialize_any(Check)
    }
}
pub(crate) fn decode(bytes: &[u8]) -> Result<Value, IrError> {
    serde_json::from_slice::<Unique>(bytes).map_err(|_| IrError::InvalidField("upstream_json"))?;
    serde_json::from_slice(bytes).map_err(|_| IrError::InvalidField("upstream_json"))
}
pub(crate) fn object(value: &Value) -> Result<&Map<String, Value>, IrError> {
    value
        .as_object()
        .ok_or(IrError::InvalidField("upstream_json"))
}
pub(crate) fn string<'a>(value: &'a Value, field: &'static str) -> Result<&'a str, IrError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or(IrError::InvalidField(field))
}
pub(crate) fn known_fields(value: &Value, fields: &[&str]) -> Result<(), IrError> {
    if object(value)?.keys().any(|k| !fields.contains(&k.as_str())) {
        return Err(IrError::UnsupportedExtension);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_keys_reject_without_rounding_scalars() {
        for input in [r#"{"a":1,"a":2}"#, r#"[{"a":{"input":"a","input":"b"}}]"#] {
            assert!(decode(input.as_bytes()).is_err());
        }
        let input = br#"{"integer":900719925474099312345,"fraction":0.1234567890123456789,"huge":1e1000,"array":[true,false,null,"text"]}"#;
        let value = decode(input).unwrap();
        assert_eq!(value["integer"].to_string(), "900719925474099312345");
        assert_eq!(value["fraction"].to_string(), "0.1234567890123456789");
        assert_eq!(value["huge"].to_string(), "1e+1000");
    }
}
