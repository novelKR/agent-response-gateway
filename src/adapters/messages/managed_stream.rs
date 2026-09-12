//! Native Messages assembly for managed execution. Only public text leaves this state early.
use super::*;
use crate::adapters::{managed::ManagedOutput, sse::SseEvent};

pub(crate) struct NativeMessagesStream<'a> {
    prepared: &'a PreparedMessages,
    message: Option<Value>,
    blocks: Vec<Value>,
    active: Option<usize>,
    arguments: String,
    signature_started: bool,
    public_count: usize,
    active_public: Option<usize>,
    stopped: bool,
    done: bool,
    limit: usize,
    bytes: usize,
    progress: Vec<Value>,
}
impl<'a> NativeMessagesStream<'a> {
    pub fn new(prepared: &'a PreparedMessages, limit: usize) -> Self {
        Self {
            prepared,
            message: None,
            blocks: vec![],
            active: None,
            arguments: String::new(),
            signature_started: false,
            public_count: 0,
            active_public: None,
            stopped: false,
            done: false,
            limit,
            bytes: 0,
            progress: vec![],
        }
    }
    fn item_id(&self, index: usize) -> Result<String, IrError> {
        Ok(format!(
            "item_{}_{}",
            string(
                self.message.as_ref().ok_or(IrError::InvalidEventOrder)?,
                "id"
            )?,
            index
        ))
    }
    fn start_public(&mut self, index: usize, reasoning: bool) -> Result<(), IrError> {
        let id = self.item_id(index)?;
        self.active_public = Some(self.public_count);
        self.public_count += 1;
        let item = if reasoning {
            json!({"type":"reasoning","id":id,"summary":[]})
        } else {
            json!({"type":"message","id":id,"role":"assistant","status":"in_progress","content":[]})
        };
        self.progress.push(json!({"type":"response.output_item.added","output_index":self.active_public,"item":item}));
        self.progress.push(if reasoning {json!({"type":"response.reasoning_summary_part.added","output_index":self.active_public,"item_id":id,"summary_index":0,"part":{"type":"summary_text","text":""}})} else {json!({"type":"response.content_part.added","output_index":self.active_public,"item_id":id,"content_index":0,"part":{"type":"output_text","text":"","annotations":[]}})});
        Ok(())
    }
    fn text(&mut self, index: usize, text: &str, thinking: bool) -> Result<(), IrError> {
        if text.is_empty() {
            return Ok(());
        }
        if self.active_public.is_none() {
            self.start_public(index, thinking)?;
        }
        let id = self.item_id(index)?;
        let event = if thinking {
            json!({"type":"response.reasoning_summary_text.delta","output_index":self.active_public,"item_id":id,"summary_index":0,"delta":text})
        } else {
            json!({"type":"response.output_text.delta","output_index":self.active_public,"item_id":id,"content_index":0,"delta":text})
        };
        if !text.is_empty() {
            self.progress.push(event);
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
        if self.done {
            return Err(IrError::InvalidEventOrder);
        }
        let v = crate::adapters::json::decode(event.data.as_bytes())?;
        let kind = string(&v, "type")?;
        if event.event != kind {
            return Err(IrError::InvalidEventOrder);
        }
        match kind {
            "ping" => known_fields(&v, &["type"]),
            "message_start" => {
                known_fields(&v, &["type", "message"])?;
                if self.message.is_some() {
                    return Err(IrError::InvalidEventOrder);
                }
                let message = v.get("message").ok_or(unsupported())?;
                known_fields(
                    message,
                    &[
                        "id",
                        "type",
                        "role",
                        "model",
                        "content",
                        "stop_reason",
                        "stop_sequence",
                        "usage",
                    ],
                )?;
                if string(message, "type")? != "message"
                    || string(message, "role")? != "assistant"
                    || string(message, "model")? != self.prepared.model
                    || !message["content"].as_array().is_some_and(Vec::is_empty)
                    || message.get("stop_reason").is_some_and(|v| !v.is_null())
                    || message.get("stop_sequence").is_some_and(|v| !v.is_null())
                {
                    return Err(unsupported());
                }
                ResponseId::new(format!("resp_{}", string(message, "id")?))?;
                managed_usage(message.get("usage"))?;
                self.message = Some(message.clone());
                self.progress.push(json!({"type":"response.created","response":{"id":format!("resp_{}",string(message,"id")?),"object":"response","status":"in_progress","output":[]}}));
                Ok(())
            }
            "content_block_start" => {
                known_fields(&v, &["type", "index", "content_block"])?;
                if self.message.is_none()
                    || self.active.is_some()
                    || self.stopped
                    || v["index"].as_u64() != Some(self.blocks.len() as u64)
                {
                    return Err(IrError::InvalidEventOrder);
                }
                let block = v.get("content_block").ok_or(unsupported())?;
                let kind = string(block, "type")?;
                match kind {
                    "thinking" => {
                        known_fields(block, &["type", "thinking", "signature"])?;
                        string(block, "thinking")?;
                        if block.get("signature").is_some_and(|v| !v.is_string()) {
                            return Err(unsupported());
                        }
                    }
                    "redacted_thinking" => {
                        known_fields(block, &["type", "data"])?;
                        if string(block, "data")?.is_empty() {
                            return Err(unsupported());
                        }
                    }
                    "text" => {
                        known_fields(block, &["type", "text", "citations"])?;
                        string(block, "text")?;
                        if block.get("citations").is_some_and(|v| {
                            !v.is_null() && !v.as_array().is_some_and(Vec::is_empty)
                        }) {
                            return Err(unsupported());
                        }
                    }
                    "tool_use" => {
                        known_fields(block, &["type", "id", "name", "input"])?;
                        CallId::new(string(block, "id")?)?;
                        self.prepared.tools.resolve(string(block, "name")?)?;
                        if !block["input"].as_object().is_some_and(Map::is_empty) {
                            return Err(IrError::InvalidJsonArguments);
                        }
                    }
                    _ => return Err(unsupported()),
                }
                let index = self.blocks.len();
                self.active = Some(index);
                self.arguments.clear();
                self.signature_started = false;
                self.active_public = None;
                if kind == "tool_use" {
                    self.public_count += 1;
                }
                self.blocks.push(block.clone());
                if matches!(kind, "thinking" | "text") {
                    let reasoning = kind == "thinking";
                    let text = string(block, if reasoning { "thinking" } else { "text" })?;
                    if !reasoning || !text.is_empty() {
                        self.start_public(index, reasoning)?;
                        self.text(index, text, reasoning)?;
                    }
                }
                Ok(())
            }
            "content_block_delta" => {
                known_fields(&v, &["type", "index", "delta"])?;
                let index = self.active.ok_or(IrError::InvalidEventOrder)?;
                if v["index"].as_u64() != Some(index as u64) {
                    return Err(IrError::InvalidEventOrder);
                }
                let delta = v.get("delta").ok_or(unsupported())?;
                let (field, public) =
                    match (string(&self.blocks[index], "type")?, string(delta, "type")?) {
                        ("thinking", "thinking_delta") if !self.signature_started => {
                            ("thinking", Some(true))
                        }
                        ("thinking", "signature_delta") => {
                            self.signature_started = true;
                            ("signature", None)
                        }
                        ("text", "text_delta") => ("text", Some(false)),
                        ("tool_use", "input_json_delta") => ("partial_json", None),
                        _ => return Err(unsupported()),
                    };
                known_fields(delta, &["type", field])?;
                let text = string(delta, field)?;
                if field == "partial_json" {
                    self.arguments.push_str(text);
                } else {
                    let old = self.blocks[index]
                        .get(field)
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    self.blocks[index][field] = json!(format!("{old}{text}"));
                }
                if let Some(thinking) = public {
                    self.text(index, text, thinking)?;
                }
                Ok(())
            }
            "content_block_stop" => {
                known_fields(&v, &["type", "index"])?;
                let index = self.active.take().ok_or(IrError::InvalidEventOrder)?;
                if v["index"].as_u64() != Some(index as u64) {
                    return Err(IrError::InvalidEventOrder);
                }
                if self.blocks[index]["type"] == "tool_use" && !self.arguments.is_empty() {
                    self.blocks[index]["input"] =
                        crate::adapters::json::decode(self.arguments.as_bytes())?;
                }
                if self.blocks[index]["type"] == "thinking"
                    && self.blocks[index]["signature"]
                        .as_str()
                        .is_none_or(str::is_empty)
                {
                    return Err(unsupported());
                }
                Ok(())
            }
            "message_delta" => {
                known_fields(&v, &["type", "delta", "usage"])?;
                if self.message.is_none() || self.active.is_some() || self.stopped {
                    return Err(IrError::InvalidEventOrder);
                }
                let delta = v.get("delta").ok_or(unsupported())?;
                known_fields(delta, &["stop_reason", "stop_sequence"])?;
                string(delta, "stop_reason")?;
                let message = self.message.as_mut().ok_or(unsupported())?;
                for (k, v) in object(delta)? {
                    message[k] = v.clone();
                }
                if let Some(usage) = v.get("usage") {
                    managed_usage(Some(usage))?;
                    if !message["usage"].is_object() {
                        message["usage"] = json!({});
                    }
                    for (k, v) in object(usage)? {
                        message["usage"][k] = v.clone();
                    }
                }
                self.stopped = true;
                Ok(())
            }
            "message_stop" => {
                known_fields(&v, &["type"])?;
                if !self.stopped || self.active.is_some() {
                    return Err(IrError::InvalidEventOrder);
                }
                self.message.as_mut().ok_or(unsupported())?["content"] = json!(self.blocks);
                self.done = true;
                Ok(())
            }
            _ => Err(unsupported()),
        }
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
        self.prepared
            .decode_managed(self.message.ok_or(unsupported())?)
    }
}
