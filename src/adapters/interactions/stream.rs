use super::*;
use crate::adapters::sse::SseEvent;
/// Reconstructs provider steps before exposing executable tool completion.
pub struct InteractionsStream<'a> {
    prepared: &'a PreparedInteractions,
    limit: usize,
    bytes: usize,
    steps: Vec<Value>,
    active: Option<usize>,
    arguments: String,
    interaction: Option<Value>,
    terminal: bool,
    done: bool,
    response_id: String,
    progress: Vec<Value>,
    cumulative_usage: Option<Value>,
}
impl<'a> InteractionsStream<'a> {
    pub(super) fn new(
        prepared: &'a PreparedInteractions,
        limit: usize,
        response_id: String,
    ) -> Self {
        Self {
            progress: Vec::new(),
            cumulative_usage: None,
            prepared,
            limit,
            bytes: 0,
            steps: vec![],
            active: None,
            arguments: String::new(),
            interaction: None,
            terminal: false,
            done: false,
            response_id,
        }
    }
    pub fn event(&mut self, event: SseEvent) -> Result<(), IrError> {
        self.bytes = self
            .bytes
            .checked_add(event.data.len())
            .ok_or(IrError::SizeLimit)?;
        if self.bytes > self.limit {
            return Err(IrError::SizeLimit);
        }
        if self.done {
            return Err(IrError::InvalidEventOrder);
        }
        if event.data == "[DONE]" {
            if !self.terminal || self.active.is_some() {
                return Err(IrError::InvalidEventOrder);
            }
            self.done = true;
            return Ok(());
        }
        if self.terminal {
            return Err(IrError::InvalidEventOrder);
        }
        let v = decode(event.data.as_bytes())?;
        let kind = string(&v, "event_type")?;
        let allowed: &[&str] = match kind {
            "interaction.created" | "interaction.completed" => {
                &["event_type", "event_id", "interaction"]
            }
            "interaction.status_update" => &["event_type", "event_id", "interaction_id", "status"],
            "step.start" => &["event_type", "event_id", "index", "step"],
            "step.delta" => &["event_type", "event_id", "index", "delta", "metadata"],
            "step.stop" => &["event_type", "event_id", "index", "step_usage", "usage"],
            _ => return Err(unsupported()),
        };
        known_fields(&v, allowed)?;
        if v.get("event_id").is_some_and(|id| !id.is_string()) {
            return Err(unsupported());
        }
        let event_copy = v.clone();
        if event.event != "message" && event.event != kind {
            return Err(IrError::InvalidEventOrder);
        }
        match kind {
            "interaction.created" => {
                if self.interaction.is_some() {
                    return Err(IrError::InvalidEventOrder);
                }
                let meta = v.get("interaction").ok_or(unsupported())?;
                known_fields(
                    meta,
                    &[
                        "id", "status", "model", "object", "created", "updated", "steps", "usage",
                    ],
                )?;
                if string(meta, "status")? != "in_progress"
                    || meta
                        .get("model")
                        .is_some_and(|v| v.as_str() != Some(self.prepared.model.as_str()))
                    || meta
                        .get("object")
                        .is_some_and(|v| v.as_str() != Some("interaction"))
                    || meta
                        .get("steps")
                        .is_some_and(|v| !v.as_array().is_some_and(Vec::is_empty))
                {
                    return Err(unsupported());
                }
                ItemId::new(string(meta, "id")?)?;
                if let Some(cumulative) = meta.get("usage") {
                    usage(Some(cumulative))?;
                    self.cumulative_usage = Some(cumulative.clone());
                }
                self.interaction = Some(meta.clone());
            }
            "interaction.status_update" => {
                let meta = self
                    .interaction
                    .as_ref()
                    .ok_or(IrError::InvalidEventOrder)?;
                if v.get("interaction_id") != meta.get("id")
                    || !matches!(
                        string(&v, "status")?,
                        "in_progress"
                            | "requires_action"
                            | "completed"
                            | "incomplete"
                            | "failed"
                            | "cancelled"
                    )
                {
                    return Err(unsupported());
                }
            }
            "step.start" => {
                if self.interaction.is_none()
                    || self.active.is_some()
                    || v["index"].as_u64() != Some(self.steps.len() as u64)
                {
                    return Err(IrError::InvalidEventOrder);
                }
                let s = v.get("step").ok_or(unsupported())?.clone();
                if !matches!(
                    string(&s, "type")?,
                    "thought" | "function_call" | "model_output"
                ) {
                    return Err(unsupported());
                }
                self.active = Some(self.steps.len());
                self.steps.push(s);
                self.arguments.clear();
            }
            "step.delta" => {
                let i = self.active.ok_or(IrError::InvalidEventOrder)?;
                if v["index"].as_u64() != Some(i as u64) {
                    return Err(IrError::InvalidEventOrder);
                }
                if let Some(metadata) = v.get("metadata") {
                    known_fields(metadata, &["total_usage"])?;
                    if let Some(cumulative) = metadata.get("total_usage") {
                        usage(Some(cumulative))?;
                        self.cumulative_usage = Some(cumulative.clone());
                    }
                }
                let d = v.get("delta").ok_or(unsupported())?;
                let step = &mut self.steps[i];
                match (string(step, "type")?, string(d, "type")?) {
                    ("thought", "thought_signature") => {
                        known_fields(d, &["type", "signature"])?;
                        let old = step.get("signature").and_then(Value::as_str).unwrap_or("");
                        step["signature"] = json!(format!("{old}{}", string(d, "signature")?));
                    }
                    ("function_call", "arguments_delta") => {
                        known_fields(d, &["type", "arguments"])?;
                        self.arguments.push_str(string(d, "arguments")?);
                    }
                    ("model_output", "text") => {
                        known_fields(d, &["type", "text"])?;
                        if step.get("content").is_none() {
                            step["content"] = json!([{"type":"text","text":""}]);
                        }
                        let content = step["content"].as_array_mut().ok_or(unsupported())?;
                        if content.is_empty() {
                            content.push(json!({"type":"text","text":""}));
                        }
                        let last = content.last_mut().ok_or(unsupported())?;
                        let old = string(last, "text")?;
                        last["text"] = json!(format!("{old}{}", string(d, "text")?));
                    }
                    _ => return Err(unsupported()),
                }
            }
            "step.stop" => {
                let i = self.active.take().ok_or(IrError::InvalidEventOrder)?;
                if v["index"].as_u64() != Some(i as u64) {
                    return Err(IrError::InvalidEventOrder);
                }
                if !self.arguments.is_empty() {
                    if self.steps[i].get("arguments").is_some() {
                        return Err(IrError::InvalidJsonArguments);
                    }
                    self.steps[i]["arguments"] = decode(self.arguments.as_bytes())?;
                }
                if let Some(per_step) = v.get("step_usage") {
                    usage(Some(per_step))?;
                }
                if let Some(cumulative) = v.get("usage") {
                    usage(Some(cumulative))?;
                    self.cumulative_usage = Some(cumulative.clone());
                }
            }
            "interaction.completed" => {
                if self.active.is_some() {
                    return Err(IrError::InvalidEventOrder);
                }
                let prior = self
                    .interaction
                    .as_ref()
                    .ok_or(IrError::InvalidEventOrder)?;
                let mut meta = v.get("interaction").ok_or(unsupported())?.clone();
                if meta.get("id") != prior.get("id") {
                    return Err(unsupported());
                }
                if meta.get("steps").is_some_and(|s| s != &json!(self.steps)) {
                    return Err(unsupported());
                }
                if meta.get("usage").is_none()
                    && let Some(cumulative) = &self.cumulative_usage
                {
                    meta["usage"] = cumulative.clone();
                }
                meta["steps"] = json!(self.steps);
                self.interaction = Some(meta);
                self.terminal = true;
            }
            _ => return Err(unsupported()),
        }
        match kind {
            "interaction.created" => self.progress.push(json!({"type":"response.created","response":{"id":self.response_id,"object":"response","status":"in_progress","output":[]}})),
            "step.start" if event_copy["step"]["type"]=="model_output" => {
                let index=self.steps.iter().take(self.steps.len()-1).filter(|s|s["type"]!="thought").count();
                let id=format!("{}_{}",self.response_id,index);
                self.progress.push(json!({"type":"response.output_item.added","output_index":index,"item":{"id":id,"type":"message","role":"assistant","status":"in_progress","content":[]}}));
                self.progress.push(json!({"type":"response.content_part.added","item_id":id,"output_index":index,"content_index":0,"part":{"type":"output_text","text":"","annotations":[]}}));
            },
            "step.delta" if event_copy["delta"]["type"]=="text" => {
                let position=self.active.ok_or(IrError::InvalidEventOrder)?;
                let index=self.steps[..position].iter().filter(|s|s["type"]!="thought").count();
                self.progress.push(json!({"type":"response.output_text.delta","item_id":format!("{}_{}",self.response_id,index),"output_index":index,"content_index":0,"delta":event_copy["delta"]["text"]}));
            },_=>{}
        }
        Ok(())
    }
    pub(crate) fn accounting(&self) -> crate::adapters::managed::Accounting {
        let empty = json!({});
        let meta = self.interaction.as_ref().unwrap_or(&empty);
        crate::adapters::managed::Accounting::new(
            gateway_usage_contract::Profile::GeminiInteractionsV1,
            if self.terminal {
                meta.get("usage")
            } else {
                self.cumulative_usage.as_ref()
            },
            meta,
            if self.terminal
                && matches!(
                    meta["status"].as_str(),
                    Some("completed" | "requires_action")
                )
            {
                gateway_usage_contract::Outcome::Completed
            } else {
                gateway_usage_contract::Outcome::InProgress
            },
        )
    }
    pub fn take_progress(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.progress)
    }
    pub fn is_complete(&self) -> bool {
        self.done
    }
    pub fn finish(self) -> Result<DecodedInteraction, IrError> {
        if !self.done || !self.terminal {
            return Err(IrError::InvalidEventOrder);
        }
        self.prepared
            .decode(self.interaction.ok_or(unsupported())?, &self.response_id)
    }
}
