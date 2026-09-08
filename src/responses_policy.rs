//! Shared stateless admission rules; no transport or IR interpretation.
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PolicyError {
    pub code: &'static str,
    pub message: &'static str,
}

fn bad(code: &'static str, message: &'static str) -> PolicyError {
    PolicyError { code, message }
}

fn bool_field(
    request: &Map<String, Value>,
    key: &'static str,
) -> Result<Option<bool>, PolicyError> {
    request
        .get(key)
        .map(|value| {
            value.as_bool().ok_or_else(|| {
                bad(
                    "invalid_request",
                    "store, background and stream must be booleans",
                )
            })
        })
        .transpose()
}

pub(crate) fn normalize_stateless(
    value: Value,
) -> Result<(Map<String, Value>, String, bool), PolicyError> {
    let Value::Object(mut request) = value else {
        return Err(bad("invalid_request", "The request must be a JSON object"));
    };
    let model = request
        .get("model")
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| bad("invalid_model", "A registered model identifier is required"))?
        .to_owned();
    if bool_field(&request, "store")? == Some(true)
        || bool_field(&request, "background")? == Some(true)
        || ["previous_response_id", "conversation", "context_management"]
            .iter()
            .any(|key| request.get(*key).is_some_and(|v| !v.is_null()))
        || request
            .get("input")
            .and_then(Value::as_array)
            .is_some_and(|items| {
                items.iter().any(|item| {
                    matches!(item.get("type").and_then(Value::as_str), Some("item_reference" | "compaction" | "compaction_trigger"))
                        // ItemReference also permits its discriminator to be omitted/null.
                        || item.as_object().is_some_and(|fields| {
                            fields.get("id").is_some_and(Value::is_string)
                                && fields.get("type").is_none_or(Value::is_null)
                                && fields.keys().all(|key| key == "id" || key == "type")
                        })
                })
            })
    {
        return Err(bad(
            "unsupported_feature",
            "Only stateless foreground requests are supported; storage, conversation history and compaction are unavailable",
        ));
    }
    let stream = bool_field(&request, "stream")?.unwrap_or(false);
    request.insert("store".into(), Value::Bool(false));
    Ok((request, model, stream))
}
