//! Dialect-specific public reasoning and original assistant state; never inferred.
use super::*;
use crate::{
    adapters::managed::ManagedOutput,
    continuation::{NativeReplay, Outcome},
    ir::reasoning::ChatDialect,
};

impl PreparedChat {
    pub(super) fn dialect(&self) -> Result<ChatDialect, IrError> {
        self.reasoning_contract
            .as_ref()
            .and_then(|c| c.chat_dialect())
            .ok_or(unsupported())
    }
    pub(super) fn public_summaries(&self, assistant: &Value) -> Result<Vec<String>, IrError> {
        let mut texts = vec![];
        match self.dialect()? {
            ChatDialect::DeepSeek => {
                if let Some(value) = assistant.get("reasoning_content").filter(|v| !v.is_null()) {
                    let text = value.as_str().ok_or(unsupported())?;
                    if !text.is_empty() {
                        texts.push(text.to_owned());
                    }
                }
            }
            ChatDialect::OpenRouter => {
                if let Some(details) = assistant.get("reasoning_details").filter(|v| !v.is_null()) {
                    let details = details.as_array().ok_or(unsupported())?;
                    let mut ids = std::collections::BTreeSet::new();
                    for (position, detail) in details.iter().enumerate() {
                        let kind = string(detail, "type")?;
                        let field = match kind {
                            "reasoning.text" => "text",
                            "reasoning.summary" => "summary",
                            "reasoning.encrypted" => "data",
                            _ => return Err(unsupported()),
                        };
                        let mut allowed = vec!["type", "id", "format", "index", field];
                        if kind == "reasoning.text" {
                            allowed.push("signature");
                        }
                        known_fields(detail, &allowed)?;
                        let format = string(detail, "format")?;
                        if !self
                            .reasoning_contract
                            .as_ref()
                            .is_some_and(|c| c.accepts_format(format))
                        {
                            return Err(unsupported());
                        }
                        if detail
                            .get("index")
                            .is_some_and(|v| v.as_u64() != Some(position as u64))
                        {
                            return Err(IrError::InvalidEventOrder);
                        }
                        if let Some(id) = detail.get("id").filter(|v| !v.is_null()) {
                            let id = id.as_str().ok_or(unsupported())?;
                            ItemId::new(id)?;
                            if !ids.insert(id) {
                                return Err(IrError::InvalidIdentifier);
                            }
                        }
                        if detail
                            .get("signature")
                            .is_some_and(|v| !v.is_null() && !v.is_string())
                        {
                            return Err(unsupported());
                        }
                        let text = string(detail, field)?;
                        if kind != "reasoning.encrypted" && !text.is_empty() {
                            texts.push(text.to_owned());
                        }
                        if kind == "reasoning.encrypted" && text.is_empty() {
                            return Err(unsupported());
                        }
                    }
                } else if let Some(value) = assistant.get("reasoning").filter(|v| !v.is_null()) {
                    let text = value.as_str().ok_or(unsupported())?;
                    if !text.is_empty() {
                        texts.push(text.to_owned());
                    }
                }
                if assistant
                    .get("reasoning")
                    .is_some_and(|v| !v.is_null() && !v.is_string())
                {
                    return Err(unsupported());
                }
            }
        }
        Ok(texts)
    }
    pub(crate) fn accounting_profile(&self) -> gateway_usage_contract::Profile {
        match self
            .reasoning_contract
            .as_ref()
            .and_then(|c| c.chat_dialect())
        {
            Some(ChatDialect::DeepSeek) => gateway_usage_contract::Profile::DeepSeekV1,
            _ => gateway_usage_contract::Profile::ChatV1,
        }
    }
    pub(crate) fn decode_managed(&self, value: Value) -> Result<ManagedOutput, IrError> {
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
            ],
        )?;
        let choices = value["choices"]
            .as_array()
            .filter(|c| c.len() == 1)
            .ok_or(unsupported())?;
        known_fields(
            &choices[0],
            &[
                "index",
                "message",
                "finish_reason",
                "logprobs",
                "native_finish_reason",
            ],
        )?;
        let assistant = choices[0].get("message").ok_or(unsupported())?.clone();
        let dialect = self.dialect()?;
        let mut allowed = vec![
            "role",
            "content",
            "tool_calls",
            "refusal",
            "annotations",
            "audio",
            "function_call",
        ];
        match dialect {
            ChatDialect::DeepSeek => allowed.push("reasoning_content"),
            ChatDialect::OpenRouter => allowed.extend(["reasoning", "reasoning_details"]),
        }
        known_fields(&assistant, &allowed)?;
        let summaries = self.public_summaries(&assistant)?;
        let mut stripped = value.clone();
        let fields = stripped["choices"][0]["message"]
            .as_object_mut()
            .ok_or(unsupported())?;
        for key in ["reasoning_content", "reasoning", "reasoning_details"] {
            fields.remove(key);
        }
        if fields.get("content").and_then(Value::as_str) == Some("") {
            fields.insert("content".into(), Value::Null);
        }
        let usage = managed_usage(value.get("usage"), dialect)?;
        stripped["usage"] = Value::Null;
        let mut response = self.decode(stripped)?;
        response["usage"] = usage.clone();
        if response["status"] != "completed" {
            return Err(IrError::InvalidEventOrder);
        }
        let id = format!("rs_{}", string(&value, "id")?);
        let mut validator = EventValidator::new(EventLimits::default())?;
        validator.apply(EventIR::Started {
            id: ResponseId::new(format!("reasoning_{}", string(&value, "id")?))?,
        })?;
        let item = ItemId::new(id.clone())?;
        validator.apply(EventIR::ItemStarted {
            id: item.clone(),
            index: OutputIndex(0),
            kind: OutputKind::Reasoning,
        })?;
        for (index, text) in summaries.iter().enumerate() {
            let index = ContentIndex(u32::try_from(index).map_err(|_| IrError::SizeLimit)?);
            validator.apply(EventIR::PartStarted {
                item: item.clone(),
                index,
                kind: EventPartKind::ReasoningText,
            })?;
            for chunk in super::super::json::event_text_chunks(text) {
                validator.apply(EventIR::TextDelta {
                    item: item.clone(),
                    index,
                    text: chunk.into(),
                })?;
            }
            validator.apply(EventIR::PartFinished {
                item: item.clone(),
                index,
            })?;
        }
        validator.apply(EventIR::ItemFinished { item })?;
        validator.apply(EventIR::Finished {
            status: Terminal::Completed,
            reason: None,
        })?;
        let summary: Vec<Value> = summaries
            .iter()
            .map(|s| json!({"type":"summary_text","text":s}))
            .collect();
        // The stable first output slot carries the single gateway envelope on
        // finalization. Empty summaries represent opaque replay only.
        response["output"]
            .as_array_mut()
            .ok_or(unsupported())?
            .insert(0, json!({"type":"reasoning","id":id,"summary":summary}));
        Ok(ManagedOutput {
            accounting: crate::adapters::managed::Accounting::new(
                self.accounting_profile(),
                value.get("usage"),
                &value,
                gateway_usage_contract::Outcome::Completed,
            ),
            usage,
            response,
            native: NativeReplay::Chat {
                version: 1,
                dialect,
                assistant,
                controls: self.reasoning_controls.clone().ok_or(unsupported())?,
            },
            outcome: if choices[0]["finish_reason"] == "tool_calls" {
                Outcome::AwaitingTools
            } else {
                Outcome::Completed
            },
        })
    }
}
fn managed_usage(value: Option<&Value>, dialect: ChatDialect) -> Result<Value, IrError> {
    let empty = json!({});
    let value = value.filter(|v| !v.is_null()).unwrap_or(&empty);
    object(value)?;
    let number = |v: Option<&Value>| -> Result<Option<u64>, IrError> {
        match v {
            None | Some(Value::Null) => Ok(None),
            Some(v) => v.as_u64().map(Some).ok_or(unsupported()),
        }
    };
    let input = number(value.get("prompt_tokens"))?;
    let output = number(value.get("completion_tokens"))?;
    let total = number(value.get("total_tokens"))?;
    if let (Some(i), Some(o), Some(t)) = (input, output, total)
        && i.checked_add(o) != Some(t)
    {
        return Err(unsupported());
    }
    for key in ["prompt_tokens_details", "completion_tokens_details"] {
        if let Some(v) = value.get(key).filter(|v| !v.is_null()) {
            object(v)?;
        }
    }
    let mut cached = number(
        value
            .get("prompt_tokens_details")
            .and_then(|v| v.get("cached_tokens")),
    )?;
    let reasoning = number(
        value
            .get("completion_tokens_details")
            .and_then(|v| v.get("reasoning_tokens")),
    )?;
    if dialect == ChatDialect::DeepSeek {
        let hit = number(value.get("prompt_cache_hit_tokens"))?;
        let miss = number(value.get("prompt_cache_miss_tokens"))?;
        if let (Some(hit), Some(miss), Some(input)) = (hit, miss, input)
            && hit.checked_add(miss) != Some(input)
        {
            return Err(unsupported());
        }
        if let Some(hit) = hit {
            if cached.is_some_and(|v| v != hit) {
                return Err(unsupported());
            }
            cached = Some(hit);
        }
    }
    if cached.zip(input).is_some_and(|(c, i)| c > i)
        || reasoning.zip(output).is_some_and(|(r, o)| r > o)
    {
        return Err(unsupported());
    }
    let mut result = json!({"input_tokens":input,"output_tokens":output,"total_tokens":total});
    if let Some(cached) = cached {
        result["input_tokens_details"] = json!({"cached_tokens":cached});
    }
    if let Some(reasoning) = reasoning {
        result["output_tokens_details"] = json!({"reasoning_tokens":reasoning});
    }
    Ok(result)
}
