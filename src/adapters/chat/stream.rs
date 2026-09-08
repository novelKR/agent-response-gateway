//! Incremental Chat stream framing with canonical assistant-text-before-tools projection.
use super::*;
use crate::adapters::{json::event_text_chunks, sse::SseEvent};
use crate::ir::{ToolIdentity, ToolKind};

#[derive(Default)]
struct ToolFragments {
    id: String,
    name: String,
    arguments: String,
    function_type: bool,
}
pub struct ChatStream<'a> {
    prepared: &'a PreparedChat,
    validator: EventValidator,
    id: Option<String>,
    created: Option<u64>,
    text: Option<String>,
    calls: BTreeMap<usize, ToolFragments>,
    finish_reason: Option<String>,
    final_response: Option<Value>,
    usage: Option<Value>,
    buffered: usize,
    argument_bytes: usize,
    limit: usize,
    sequence: u64,
    failed: bool,
    done: bool,
}
impl PreparedChat {
    pub fn stream(&self, max_output_bytes: usize) -> Result<ChatStream<'_>, IrError> {
        if max_output_bytes == 0 {
            return Err(IrError::SizeLimit);
        }
        Ok(ChatStream {
            prepared: self,
            validator: EventValidator::new(EventLimits::default())?,
            id: None,
            created: None,
            text: None,
            calls: BTreeMap::new(),
            finish_reason: None,
            final_response: None,
            usage: None,
            buffered: 0,
            argument_bytes: 0,
            limit: max_output_bytes,
            sequence: 0,
            failed: false,
            done: false,
        })
    }
}
impl ChatStream<'_> {
    pub fn event(&mut self, event: SseEvent) -> Result<Vec<Value>, IrError> {
        if self.failed || self.done {
            return Err(IrError::InvalidEventOrder);
        }
        let result = self.convert(event);
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    pub fn is_complete(&self) -> bool {
        self.done && !self.failed
    }
    pub fn finish(&self) -> Result<(), IrError> {
        if self.is_complete() {
            Ok(())
        } else {
            Err(IrError::InvalidEventOrder)
        }
    }
    fn emit(&mut self, kind: &str, mut value: Value, output: &mut Vec<Value>) {
        value["type"] = json!(kind);
        value["sequence_number"] = json!(self.sequence);
        self.sequence += 1;
        output.push(value);
    }
    fn reserve(&mut self, bytes: usize) -> Result<(), IrError> {
        if bytes > self.limit.saturating_sub(self.buffered) {
            return Err(IrError::SizeLimit);
        }
        self.buffered += bytes;
        Ok(())
    }
    fn convert(&mut self, event: SseEvent) -> Result<Vec<Value>, IrError> {
        if event.event != "message" {
            return Err(IrError::InvalidField("chat_stream_event"));
        }
        if event.data.len() > self.limit {
            return Err(IrError::SizeLimit);
        }
        let mut output = Vec::new();
        if event.data == "[DONE]" {
            let finish = self
                .finish_reason
                .as_ref()
                .ok_or(IrError::InvalidEventOrder)?;
            let (terminal, reason) = self.prepared.terminal(finish, self.calls.len())?;
            let mut response = self
                .final_response
                .take()
                .ok_or(IrError::InvalidEventOrder)?;
            response["usage"] = self.usage.clone().unwrap_or(Value::Null);
            self.validator.apply(EventIR::Finished {
                status: terminal,
                reason: reason.map(str::to_owned),
            })?;
            self.emit(
                if terminal == Terminal::Completed {
                    "response.completed"
                } else {
                    "response.incomplete"
                },
                json!({"response":response}),
                &mut output,
            );
            self.done = true;
            return Ok(output);
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
                "obfuscation",
                "metadata",
                "moderation",
            ],
        )?;
        for field in ["metadata", "moderation"] {
            if value.get(field).is_some_and(|v| !v.is_null()) {
                return Err(unsupported());
            }
        }
        if string(&value, "object")? != "chat.completion.chunk"
            || string(&value, "model")? != self.prepared.model
        {
            return Err(IrError::InvalidField("chat_chunk"));
        }
        let id = string(&value, "id")?;
        let created = value
            .get("created")
            .and_then(Value::as_u64)
            .ok_or(IrError::InvalidField("created"))?;
        if let Some(existing) = &self.id {
            if existing != id || self.created != Some(created) {
                return Err(IrError::InvalidField("chat_chunk_identity"));
            }
        } else {
            let response = ResponseId::new(format!("resp_{id}"))?;
            self.validator.apply(EventIR::Started {
                id: response.clone(),
            })?;
            self.id = Some(id.into());
            self.created = Some(created);
            self.emit("response.created",json!({"response":{"id":response.as_str(),"object":"response","created_at":created,"model":self.prepared.model,"status":"in_progress","output":[],"usage":Value::Null,"error":Value::Null,"incomplete_details":Value::Null}}),&mut output);
        }
        let choices = value
            .get("choices")
            .and_then(Value::as_array)
            .ok_or(IrError::InvalidField("choices"))?;
        if choices.is_empty() {
            if self.finish_reason.is_none() || value.get("usage").is_none_or(Value::is_null) {
                return Err(IrError::InvalidEventOrder);
            }
        } else {
            if choices.len() != 1 || self.finish_reason.is_some() {
                return Err(IrError::InvalidEventOrder);
            }
            let choice = &choices[0];
            known_fields(choice, &["index", "delta", "finish_reason", "logprobs"])?;
            if choice.get("index").and_then(Value::as_u64) != Some(0)
                || choice.get("logprobs").is_some_and(|v| !v.is_null())
            {
                return Err(unsupported());
            }
            let delta = choice.get("delta").ok_or(IrError::InvalidField("delta"))?;
            known_fields(
                delta,
                &[
                    "role",
                    "content",
                    "tool_calls",
                    "refusal",
                    "annotations",
                    "audio",
                    "function_call",
                ],
            )?;
            if delta
                .get("role")
                .is_some_and(|v| !v.is_null() && v.as_str() != Some("assistant"))
            {
                return Err(IrError::InvalidField("role"));
            }
            for field in ["audio", "function_call"] {
                if delta.get(field).is_some_and(|v| !v.is_null()) {
                    return Err(unsupported());
                }
            }
            if delta
                .get("refusal")
                .is_some_and(|v| !v.is_null() && v.as_str() != Some(""))
                || delta
                    .get("annotations")
                    .is_some_and(|v| !v.is_null() && !v.as_array().is_some_and(Vec::is_empty))
            {
                return Err(unsupported());
            }
            if let Some(text) = delta.get("content").filter(|v| !v.is_null()) {
                self.text_delta(text.as_str().ok_or(unsupported())?, &mut output)?;
            }
            if let Some(calls) = delta.get("tool_calls").filter(|v| !v.is_null()) {
                for call in calls
                    .as_array()
                    .ok_or(IrError::InvalidField("tool_calls"))?
                {
                    self.tool_delta(call)?;
                }
            }
            if let Some(finish) = choice.get("finish_reason").filter(|v| !v.is_null()) {
                let finish = finish
                    .as_str()
                    .ok_or(IrError::InvalidField("finish_reason"))?;
                self.finish_reason = Some(finish.into());
                self.flush(&mut output)?;
            }
        }
        if let Some(value) = value.get("usage").filter(|v| !v.is_null()) {
            let usage = usage(value)?;
            self.validator.apply(EventIR::UsageUpdated(Usage {
                input_tokens: usage["input_tokens"].as_u64(),
                output_tokens: usage["output_tokens"].as_u64(),
            }))?;
            self.usage = Some(usage);
        }
        Ok(output)
    }
    fn text_delta(&mut self, text: &str, output: &mut Vec<Value>) -> Result<(), IrError> {
        self.reserve(text.len())?;
        let item = ItemId::new(format!("item_{}_0", self.id.as_ref().expect("started")))?;
        if self.text.is_none() {
            if self.calls.len() >= EventLimits::default().max_items {
                return Err(IrError::SizeLimit);
            }
            self.validator.apply(EventIR::ItemStarted {
                id: item.clone(),
                index: OutputIndex(0),
                kind: OutputKind::Message,
            })?;
            self.validator.apply(EventIR::PartStarted {
                item: item.clone(),
                index: ContentIndex(0),
                kind: EventPartKind::Text,
            })?;
            self.text = Some(String::new());
            self.emit("response.output_item.added",json!({"output_index":0,"item":{"id":item.as_str(),"type":"message","role":"assistant","status":"in_progress","content":[]}}),output);
            self.emit("response.content_part.added",json!({"item_id":item.as_str(),"output_index":0,"content_index":0,"part":{"type":"output_text","text":"","annotations":[]}}),output);
        }
        self.validator.apply(EventIR::TextDelta {
            item: item.clone(),
            index: ContentIndex(0),
            text: text.into(),
        })?;
        self.text.as_mut().expect("started text").push_str(text);
        if !text.is_empty() {
            self.emit("response.output_text.delta",json!({"item_id":item.as_str(),"output_index":0,"content_index":0,"delta":text,"logprobs":[]}),output);
        }
        Ok(())
    }
    fn tool_delta(&mut self, delta: &Value) -> Result<(), IrError> {
        known_fields(delta, &["index", "id", "type", "function"])?;
        let index = delta
            .get("index")
            .and_then(Value::as_u64)
            .and_then(|v| usize::try_from(v).ok())
            .filter(|v| *v < EventLimits::default().max_items)
            .ok_or(IrError::InvalidField("tool_index"))?;
        if !self.calls.contains_key(&index)
            && self.calls.len() + usize::from(self.text.is_some())
                >= EventLimits::default().max_items
        {
            return Err(IrError::SizeLimit);
        }
        let function = delta.get("function").filter(|v| !v.is_null());
        if let Some(function) = function {
            known_fields(function, &["name", "arguments"])?;
        }
        let id = fragment(delta.get("id"))?;
        let name = fragment(function.and_then(|v| v.get("name")))?;
        let arguments = fragment(function.and_then(|v| v.get("arguments")))?;
        let bytes =
            id.map_or(0, str::len) + name.map_or(0, str::len) + arguments.map_or(0, str::len);
        self.reserve(bytes)?;
        if let Some(arguments) = arguments {
            if arguments.len() > EventLimits::default().max_delta_bytes
                || arguments.len()
                    > EventLimits::default()
                        .max_argument_bytes
                        .saturating_sub(self.argument_bytes)
            {
                return Err(IrError::SizeLimit);
            }
            self.argument_bytes += arguments.len();
        }
        let call = self.calls.entry(index).or_default();
        if let Some(kind) = delta.get("type").filter(|v| !v.is_null()) {
            if kind.as_str() != Some("function") {
                return Err(unsupported());
            }
            call.function_type = true;
        }
        if let Some(id) = id {
            if id.len() > 512_usize.saturating_sub(call.id.len()) {
                return Err(IrError::SizeLimit);
            }
            call.id.push_str(id);
        }
        if let Some(name) = name {
            if name.len() > 64_usize.saturating_sub(call.name.len()) {
                return Err(IrError::SizeLimit);
            }
            call.name.push_str(name);
        }
        if let Some(arguments) = arguments {
            call.arguments.push_str(arguments);
        }
        Ok(())
    }
    fn flush(&mut self, output: &mut Vec<Value>) -> Result<(), IrError> {
        let mut calls = Vec::new();
        for (expected, (index, call)) in self.calls.iter().enumerate() {
            if expected != *index || !call.function_type {
                return Err(IrError::InvalidEventOrder);
            }
            calls.push(json!({"id":call.id,"type":"function","function":{"name":call.name,"arguments":call.arguments}}));
        }
        // A Chat response has one assistant content field and a separate ordered tool array.
        // Buffer tools until finish so late text does not change already emitted item indices.
        let source = json!({"id":self.id,"object":"chat.completion","created":self.created,"model":self.prepared.model,
            "choices":[{"index":0,"message":{"role":"assistant","content":self.text,"tool_calls":calls},"finish_reason":self.finish_reason}],"usage":Value::Null});
        let response = self.prepared.decode(source)?;
        for (index, item) in response["output"]
            .as_array()
            .expect("decoded output")
            .iter()
            .enumerate()
        {
            let id = ItemId::new(item["id"].as_str().expect("decoded id"))?;
            if item["type"] == "message" {
                self.validator.apply(EventIR::PartFinished {
                    item: id.clone(),
                    index: ContentIndex(0),
                })?;
                let part = &item["content"][0];
                self.emit("response.output_text.done",json!({"item_id":id.as_str(),"output_index":index,"content_index":0,"text":part["text"],"logprobs":[]}),output);
                self.emit("response.content_part.done",json!({"item_id":id.as_str(),"output_index":index,"content_index":0,"part":part}),output);
            } else {
                let custom = item["type"] == "custom_tool_call";
                let argument = if custom { "input" } else { "arguments" };
                let prefix = if custom {
                    "response.custom_tool_call_input"
                } else {
                    "response.function_call_arguments"
                };
                self.validator.apply(EventIR::ItemStarted {
                    id: id.clone(),
                    index: OutputIndex(index as u32),
                    kind: OutputKind::Tool {
                        tool: ToolIdentity::new(
                            item.get("namespace")
                                .and_then(Value::as_str)
                                .map(str::to_owned),
                            item["name"].as_str().expect("decoded name"),
                        )?,
                        call_id: CallId::new(item["call_id"].as_str().expect("decoded call id"))?,
                        kind: if custom {
                            ToolKind::Custom
                        } else {
                            ToolKind::Function
                        },
                    },
                })?;
                let mut partial = item.clone();
                partial["status"] = json!("in_progress");
                partial[argument] = json!("");
                self.emit(
                    "response.output_item.added",
                    json!({"output_index":index,"item":partial}),
                    output,
                );
                let text = item[argument].as_str().expect("decoded arguments");
                for chunk in event_text_chunks(text) {
                    self.validator.apply(EventIR::ArgumentsDelta {
                        item: id.clone(),
                        text: chunk.into(),
                    })?;
                    self.emit(
                        &format!("{prefix}.delta"),
                        json!({"item_id":id.as_str(),"output_index":index,"delta":chunk}),
                        output,
                    );
                }
                let mut done = json!({"item_id":id.as_str(),"output_index":index});
                done[argument] = json!(text);
                self.emit(&format!("{prefix}.done"), done, output);
            }
            self.validator.apply(EventIR::ItemFinished { item: id })?;
            self.emit(
                "response.output_item.done",
                json!({"output_index":index,"item":item}),
                output,
            );
        }
        self.final_response = Some(response);
        Ok(())
    }
}

fn fragment(value: Option<&Value>) -> Result<Option<&str>, IrError> {
    value
        .filter(|v| !v.is_null())
        .map(|v| v.as_str().ok_or(IrError::InvalidField("tool_fragment")))
        .transpose()
}
