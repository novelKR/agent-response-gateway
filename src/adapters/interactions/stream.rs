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
}
impl<'a> InteractionsStream<'a> {
    pub(super) fn new(
        prepared: &'a PreparedInteractions,
        limit: usize,
        response_id: String,
    ) -> Self {
        Self {
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
        if event.event != "message" && event.event != kind {
            return Err(IrError::InvalidEventOrder);
        }
        match kind {
            "interaction.created" => {
                if self.interaction.is_some() {
                    return Err(IrError::InvalidEventOrder);
                }
                let meta = v.get("interaction").ok_or(unsupported())?;
                if string(meta, "status")? != "in_progress"
                    || string(meta, "model")? != self.prepared.model
                {
                    return Err(unsupported());
                }
                self.interaction = Some(meta.clone());
            }
            "interaction.status_update" => {
                let meta = self
                    .interaction
                    .as_ref()
                    .ok_or(IrError::InvalidEventOrder)?;
                if v.get("interaction_id") != meta.get("id")
                    || !matches!(string(&v, "status")?, "in_progress" | "requires_action")
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
                    self.steps[i]["arguments"] = decode(self.arguments.as_bytes())?;
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
                meta["steps"] = json!(self.steps);
                self.interaction = Some(meta);
                self.terminal = true;
            }
            _ => return Err(unsupported()),
        }
        Ok(())
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
