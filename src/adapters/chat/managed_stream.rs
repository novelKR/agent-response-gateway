//! Chat dialect stream assembly. Identity and native reasoning remain provider-owned.
use super::*;
use crate::{
    adapters::{managed::ManagedOutput, sse::SseEvent},
    ir::reasoning::ChatDialect,
};

pub(crate) struct NativeChatStream<'a> {
    prepared: &'a PreparedChat,
    meta: Value,
    assistant: Value,
    calls: Vec<Value>,
    details: Vec<Value>,
    finish: Option<String>,
    usage: Option<Value>,
    done: bool,
    started: bool,
    text_started: bool,
    shown_text: String,
    shown_reasoning: Vec<String>,
    progress: Vec<Value>,
    bytes: usize,
    limit: usize,
}
fn immutable(object: &mut Value, key: &str, value: &Value) -> Result<(), IrError> {
    if value.is_null() {
        if object.get(key).is_none() {
            object[key] = Value::Null;
        }
        return Ok(());
    }
    if object
        .get(key)
        .is_some_and(|old| !old.is_null() && old != value)
    {
        return Err(IrError::InvalidIdentifier);
    }
    object[key] = value.clone();
    Ok(())
}
fn append(object: &mut Value, key: &str, value: &Value) -> Result<(), IrError> {
    if value.is_null() {
        if object.get(key).is_none() {
            object[key] = Value::Null;
        }
        return Ok(());
    }
    let text = value.as_str().ok_or(unsupported())?;
    let old = object
        .get(key)
        .filter(|v| !v.is_null())
        .map(|v| v.as_str().ok_or(unsupported()))
        .transpose()?
        .unwrap_or("");
    object[key] = json!(format!("{old}{text}"));
    Ok(())
}
impl<'a> NativeChatStream<'a> {
    pub fn new(prepared: &'a PreparedChat, limit: usize) -> Self {
        Self {
            prepared,
            meta: json!({}),
            assistant: json!({}),
            calls: vec![],
            details: vec![],
            finish: None,
            usage: None,
            done: false,
            started: false,
            text_started: false,
            shown_text: String::new(),
            shown_reasoning: vec![],
            progress: vec![],
            bytes: 0,
            limit,
        }
    }
    fn identity_complete(&self) -> bool {
        self.meta["id"].is_string()
            && self.meta["model"].is_string()
            && self.meta["created"].is_u64()
    }
    fn publish_progress(&mut self, terminal: bool) -> Result<(), IrError> {
        if !self.identity_complete() {
            return Ok(());
        }
        let provider_id = string(&self.meta, "id")?;
        let response_id = format!("resp_{provider_id}");
        let reasoning_id = format!("rs_{provider_id}");
        let text_id = format!("item_{provider_id}_0");
        if !self.started {
            self.progress.push(json!({"type":"response.created","response":{"id":response_id,"object":"response","status":"in_progress","output":[]}}));
            self.progress.push(json!({"type":"response.output_item.added","output_index":0,"item":{"type":"reasoning","id":reasoning_id,"summary":[]}}));
            self.started = true;
        }
        // A flat OpenRouter mirror cannot be shown until we know no details follow.
        let summaries = if terminal || self.prepared.dialect()? == ChatDialect::DeepSeek {
            self.prepared.public_summaries(&self.assistant)?
        } else if self.assistant.get("reasoning_details").is_some() {
            let mut available = Vec::new();
            for detail in &self.details {
                let Some(kind) = detail["type"].as_str() else {
                    break;
                };
                let field = match kind {
                    "reasoning.text" => "text",
                    "reasoning.summary" => "summary",
                    "reasoning.encrypted" => "data",
                    _ => return Err(unsupported()),
                };
                let Some(format) = detail["format"].as_str() else {
                    break;
                };
                if !self
                    .prepared
                    .reasoning_contract
                    .as_ref()
                    .is_some_and(|c| c.accepts_format(format))
                {
                    return Err(unsupported());
                }
                if detail.get(field).is_none() {
                    break;
                }
                if kind == "reasoning.encrypted" && detail[field].as_str() == Some("") {
                    break;
                }
                available.push(detail.clone());
            }
            self.prepared
                .public_summaries(&json!({"reasoning_details":available}))?
        } else {
            vec![]
        };
        for (i, text) in summaries.iter().enumerate() {
            if i == self.shown_reasoning.len() {
                self.shown_reasoning.push(String::new());
                self.progress.push(json!({"type":"response.reasoning_summary_part.added","item_id":reasoning_id,"output_index":0,"summary_index":i,"part":{"type":"summary_text","text":""}}));
            }
            let old = &self.shown_reasoning[i];
            let delta = text.strip_prefix(old).ok_or(IrError::InvalidEventOrder)?;
            if !delta.is_empty() {
                self.progress.push(json!({"type":"response.reasoning_summary_text.delta","item_id":reasoning_id,"output_index":0,"summary_index":i,"delta":delta}));
            }
            self.shown_reasoning[i] = text.clone();
        }
        if let Some(text) = self.assistant.get("content").filter(|v| !v.is_null()) {
            let text = text.as_str().ok_or(unsupported())?;
            if !text.is_empty() && !self.text_started {
                self.progress.push(json!({"type":"response.output_item.added","output_index":1,"item":{"type":"message","id":text_id,"role":"assistant","status":"in_progress","content":[]}}));
                self.progress.push(json!({"type":"response.content_part.added","item_id":text_id,"output_index":1,"content_index":0,"part":{"type":"output_text","text":"","annotations":[]}}));
                self.text_started = true;
            }
            let delta = text
                .strip_prefix(&self.shown_text)
                .ok_or(IrError::InvalidEventOrder)?;
            if !delta.is_empty() {
                self.progress.push(json!({"type":"response.output_text.delta","item_id":text_id,"output_index":1,"content_index":0,"delta":delta}));
            }
            self.shown_text = text.to_owned();
        }
        Ok(())
    }
    pub fn event(&mut self, event: SseEvent) -> Result<(), IrError> {
        self.bytes = self
            .bytes
            .checked_add(event.data.len())
            .ok_or(IrError::SizeLimit)?;
        if self.bytes > self.limit {
            return Err(IrError::SizeLimit);
        }
        if self.done || event.event != "message" {
            return Err(IrError::InvalidEventOrder);
        }
        if event.data == "[DONE]" {
            if self.finish.is_none() || !self.identity_complete() {
                return Err(IrError::InvalidEventOrder);
            }
            self.assemble_assistant()?;
            // Validate the whole native response before publishing buffered reasoning.
            self.prepared.decode_managed(self.response()?)?;
            self.publish_progress(true)?;
            self.done = true;
            return Ok(());
        }
        let value = crate::adapters::json::decode(event.data.as_bytes())?;
        known_fields(
            &value,
            &[
                "id",
                "object",
                "created",
                "model",
                "choices",
                "usage",
                "system_fingerprint",
                "service_tier",
                "provider",
                "obfuscation",
            ],
        )?;
        if value
            .get("object")
            .is_some_and(|v| v != "chat.completion.chunk")
        {
            return Err(unsupported());
        }
        for key in ["id", "model", "created"] {
            if let Some(v) = value.get(key).filter(|v| !v.is_null()) {
                if (key == "created" && !v.is_u64()) || (key != "created" && !v.is_string()) {
                    return Err(unsupported());
                }
                immutable(&mut self.meta, key, v)?;
            }
        }
        if self
            .meta
            .get("model")
            .is_some_and(|m| m.as_str() != Some(self.prepared.model.as_str()))
        {
            return Err(IrError::InvalidIdentifier);
        }
        if let Some(id) = self.meta.get("id") {
            ResponseId::new(format!("resp_{}", id.as_str().ok_or(unsupported())?))?;
        }
        if let Some(usage) = value.get("usage").filter(|v| !v.is_null()) {
            object(usage)?;
            for key in [
                "prompt_tokens",
                "completion_tokens",
                "total_tokens",
                "prompt_cache_hit_tokens",
                "prompt_cache_miss_tokens",
            ] {
                if usage.get(key).is_some_and(|v| !v.is_null() && !v.is_u64()) {
                    return Err(unsupported());
                }
            }
            for key in ["prompt_tokens_details", "completion_tokens_details"] {
                if usage
                    .get(key)
                    .is_some_and(|v| !v.is_null() && !v.is_object())
                {
                    return Err(unsupported());
                }
            }
            let stored = self.usage.get_or_insert_with(|| json!({}));
            for (key, v) in object(usage)? {
                if v.is_null() {
                    continue;
                }
                if v.is_object() {
                    if stored
                        .get(key)
                        .is_some_and(|old| !old.is_null() && !old.is_object())
                    {
                        return Err(unsupported());
                    }
                    if stored.get(key).is_none_or(Value::is_null) {
                        stored[key] = json!({});
                    }
                    for (nested, count) in object(v)? {
                        if count.is_null() {
                            continue;
                        }
                        if stored[key]
                            .get(nested)
                            .and_then(Value::as_u64)
                            .zip(count.as_u64())
                            .is_some_and(|(old, new)| new < old)
                        {
                            return Err(unsupported());
                        }
                        stored[key][nested] = count.clone();
                    }
                } else {
                    if stored
                        .get(key)
                        .and_then(Value::as_u64)
                        .zip(v.as_u64())
                        .is_some_and(|(old, new)| new < old)
                    {
                        return Err(unsupported());
                    }
                    stored[key] = v.clone();
                }
            }
        }
        let choices = value["choices"].as_array().ok_or(unsupported())?;
        if choices.is_empty() {
            if value.get("usage").is_none_or(Value::is_null) {
                return Err(unsupported());
            }
            return self.publish_progress(false);
        }
        if choices.len() != 1 || choices[0]["index"].as_u64() != Some(0) {
            return Err(unsupported());
        }
        let choice = &choices[0];
        known_fields(
            choice,
            &[
                "index",
                "delta",
                "finish_reason",
                "logprobs",
                "native_finish_reason",
            ],
        )?;
        if choice.get("logprobs").is_some_and(|v| !v.is_null()) {
            return Err(unsupported());
        }
        let delta = choice.get("delta").ok_or(unsupported())?;
        object(delta)?;
        if self.finish.is_some() {
            return Err(IrError::InvalidEventOrder);
        }
        let mut allowed = vec![
            "role",
            "content",
            "tool_calls",
            "refusal",
            "annotations",
            "audio",
            "function_call",
        ];
        match self.prepared.dialect()? {
            ChatDialect::DeepSeek => allowed.push("reasoning_content"),
            ChatDialect::OpenRouter => allowed.extend(["reasoning", "reasoning_details"]),
        }
        known_fields(delta, &allowed)?;
        if let Some(role) = delta.get("role").filter(|v| !v.is_null()) {
            if role != "assistant" {
                return Err(unsupported());
            }
            immutable(&mut self.assistant, "role", role)?;
        }
        for key in ["content", "reasoning", "reasoning_content"] {
            if let Some(v) = delta.get(key) {
                append(&mut self.assistant, key, v)?;
            }
        }
        for key in ["refusal", "annotations", "audio", "function_call"] {
            if let Some(v) = delta.get(key) {
                if !v.is_null()
                    && v.as_str() != Some("")
                    && !v.as_array().is_some_and(Vec::is_empty)
                {
                    return Err(unsupported());
                }
                immutable(&mut self.assistant, key, v)?;
            }
        }
        if let Some(details) = delta.get("reasoning_details").filter(|v| !v.is_null()) {
            self.assistant["reasoning_details"] = json!([]);
            for d in details.as_array().ok_or(unsupported())? {
                known_fields(
                    d,
                    &[
                        "index",
                        "id",
                        "type",
                        "format",
                        "text",
                        "summary",
                        "data",
                        "signature",
                    ],
                )?;
                let i = usize::try_from(d["index"].as_u64().ok_or(unsupported())?)
                    .map_err(|_| IrError::SizeLimit)?;
                if i > self.details.len() || i + 1 < self.details.len() {
                    return Err(IrError::InvalidEventOrder);
                }
                if i == self.details.len() {
                    self.details.push(json!({"index":i}));
                }
                let stored = &mut self.details[i];
                for key in ["id", "type", "format"] {
                    if let Some(v) = d.get(key) {
                        if !v.is_null() && !v.is_string() {
                            return Err(unsupported());
                        }
                        immutable(stored, key, v)?;
                    }
                }
                for key in ["text", "summary", "data", "signature"] {
                    if let Some(v) = d.get(key) {
                        append(stored, key, v)?;
                    }
                }
            }
        }
        if let Some(calls) = delta.get("tool_calls").filter(|v| !v.is_null()) {
            for call in calls.as_array().ok_or(unsupported())? {
                known_fields(call, &["index", "id", "type", "function"])?;
                let i = usize::try_from(call["index"].as_u64().ok_or(unsupported())?)
                    .map_err(|_| IrError::SizeLimit)?;
                if i > self.calls.len() {
                    return Err(IrError::InvalidEventOrder);
                }
                if i == self.calls.len() {
                    self.calls.push(json!({"function":{}}));
                }
                let stored = &mut self.calls[i];
                for key in ["id", "type"] {
                    if let Some(v) = call.get(key) {
                        immutable(stored, key, v)?;
                    }
                }
                if let Some(function) = call.get("function") {
                    known_fields(function, &["name", "arguments"])?;
                    for key in ["name", "arguments"] {
                        if let Some(v) = function.get(key) {
                            append(&mut stored["function"], key, v)?;
                        }
                    }
                }
            }
        }
        if let Some(finish) = choice.get("finish_reason").filter(|v| !v.is_null()) {
            self.finish = Some(finish.as_str().ok_or(unsupported())?.to_owned());
        }
        self.publish_progress(false)
    }
    fn assemble_assistant(&mut self) -> Result<(), IrError> {
        if self.assistant.get("role").is_none() {
            return Err(IrError::InvalidIdentifier);
        }
        if !self.calls.is_empty() {
            self.assistant["tool_calls"] = json!(self.calls);
        }
        if self.assistant.get("reasoning_details").is_some() {
            self.assistant["reasoning_details"] = json!(self.details);
        }
        Ok(())
    }
    fn response(&self) -> Result<Value, IrError> {
        Ok(
            json!({"id":self.meta["id"],"model":self.meta["model"],"created":self.meta["created"],"object":"chat.completion","usage":self.usage,
            "choices":[{"index":0,"message":self.assistant,"finish_reason":self.finish.as_ref().ok_or(IrError::InvalidEventOrder)?}]}),
        )
    }
    pub fn take_progress(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.progress)
    }
    pub fn is_complete(&self) -> bool {
        self.done
    }
    pub fn finish(self) -> Result<ManagedOutput, IrError> {
        if !self.done {
            return Err(IrError::InvalidEventOrder);
        }
        self.prepared.decode_managed(self.response()?)
    }
}
