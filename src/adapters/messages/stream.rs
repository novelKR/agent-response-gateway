//! Incremental Messages events projected into Responses events with bounded accumulation.
use super::*;
use crate::adapters::{json::event_text_chunks, sse::SseEvent};

struct Block {
    id: ItemId,
    open: bool,
    kind: BlockKind,
    output: Value,
}
enum BlockKind {
    Text,
    Tool {
        alias: String,
        call: CallId,
        kind: ToolKind,
        raw: String,
    },
}

pub struct MessagesStream<'a> {
    prepared: &'a PreparedMessages,
    validator: EventValidator,
    response: Option<Value>,
    blocks: Vec<Block>,
    stop: Option<String>,
    usage: Value,
    sequence: u64,
    buffered: usize,
    limit: usize,
    failed: bool,
    done: bool,
}
impl PreparedMessages {
    /// The caller owns HTTP cancellation and drops this state with the upstream stream.
    pub fn stream(&self, max_output_bytes: usize) -> Result<MessagesStream<'_>, IrError> {
        if max_output_bytes == 0 {
            return Err(IrError::SizeLimit);
        }
        Ok(MessagesStream {
            prepared: self,
            validator: EventValidator::new(EventLimits::default())?,
            response: None,
            blocks: Vec::new(),
            stop: None,
            usage: Value::Null,
            sequence: 0,
            buffered: 0,
            limit: max_output_bytes,
            failed: false,
            done: false,
        })
    }
}
impl MessagesStream<'_> {
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
    pub fn finish(&self) -> Result<(), IrError> {
        if self.done && !self.failed {
            Ok(())
        } else {
            Err(IrError::InvalidEventOrder)
        }
    }
    pub fn is_complete(&self) -> bool {
        self.done && !self.failed
    }
    fn emit(&mut self, kind: &str, mut event: Value, output: &mut Vec<Value>) {
        event["type"] = json!(kind);
        event["sequence_number"] = json!(self.sequence);
        self.sequence += 1;
        output.push(event);
    }
    fn reserve(&mut self, bytes: usize) -> Result<(), IrError> {
        if bytes > self.limit.saturating_sub(self.buffered) {
            return Err(IrError::SizeLimit);
        }
        self.buffered += bytes;
        Ok(())
    }
    fn convert(&mut self, event: SseEvent) -> Result<Vec<Value>, IrError> {
        if event.data.len() > self.limit {
            return Err(IrError::SizeLimit);
        }
        let value = crate::adapters::json::decode(event.data.as_bytes())?;
        let kind = string(&value, "type")?;
        if event.event != kind {
            return Err(IrError::InvalidField("messages_event"));
        }
        let mut output = Vec::new();
        if kind == "ping" {
            known_fields(&value, &["type"])?;
            return Ok(output);
        }
        if kind == "error" {
            return Err(IrError::InvalidField("upstream_stream_error"));
        }
        if kind == "message_start" {
            known_fields(&value, &["type", "message"])?;
            if self.response.is_some() {
                return Err(IrError::InvalidEventOrder);
            }
            let message = value
                .get("message")
                .ok_or(IrError::InvalidField("message"))?;
            if string(message, "type")? != "message"
                || string(message, "role")? != "assistant"
                || string(message, "model")? != self.prepared.model
                || !message
                    .get("content")
                    .and_then(Value::as_array)
                    .is_some_and(Vec::is_empty)
                || message.get("stop_reason").is_some_and(|v| !v.is_null())
            {
                return Err(IrError::InvalidField("message_start"));
            }
            for field in ["container", "context_management", "stop_details"] {
                if message.get(field).is_some_and(|v| !v.is_null()) {
                    return Err(unsupported());
                }
            }
            let id = ResponseId::new(format!("resp_{}", string(message, "id")?))?;
            self.usage = message
                .get("usage")
                .ok_or(IrError::InvalidField("usage"))?
                .clone();
            let (input, generated, _) = usage(&self.usage)?;
            self.validator.apply(EventIR::Started { id: id.clone() })?;
            self.validator.apply(EventIR::UsageUpdated(Usage {
                input_tokens: Some(input),
                output_tokens: Some(generated),
            }))?;
            let mut response = json!({"id":id.as_str(),"object":"response","status":"in_progress","model":self.prepared.model,
                "created_at":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_| IrError::InvalidField("response_time"))?.as_secs(),
                "output":[],"error":Value::Null,"incomplete_details":Value::Null,"usage":Value::Null});
            self.prepared.reflect_format(&mut response);
            self.response = Some(response.clone());
            self.emit(
                "response.created",
                json!({"response":response}),
                &mut output,
            );
            return Ok(output);
        }
        if self.response.is_none() {
            return Err(IrError::InvalidEventOrder);
        }
        match kind {
            "content_block_start" => {
                known_fields(&value, &["type", "index", "content_block"])?;
                if self.stop.is_some() {
                    return Err(IrError::InvalidEventOrder);
                }
                let index = index(&value)?;
                if index != self.blocks.len() {
                    return Err(IrError::InvalidEventOrder);
                }
                let block = value
                    .get("content_block")
                    .ok_or(IrError::InvalidField("content_block"))?;
                let id = ItemId::new(format!(
                    "item_{}_{}",
                    self.response.as_ref().expect("started")["id"]
                        .as_str()
                        .expect("id")
                        .strip_prefix("resp_")
                        .expect("response prefix"),
                    index
                ))?;
                let (block_kind, item, event_kind) = match string(block, "type")? {
                    "text" => {
                        known_fields(block, &["type", "text", "citations"])?;
                        if block.get("citations").is_some_and(|v| {
                            !v.is_null() && !v.as_array().is_some_and(Vec::is_empty)
                        }) {
                            return Err(unsupported());
                        }
                        (
                            BlockKind::Text,
                            json!({"id":id.as_str(),"type":"message","role":"assistant","status":"in_progress","content":[]}),
                            OutputKind::Message,
                        )
                    }
                    "tool_use" => {
                        known_fields(block, &["type", "id", "name", "input"])?;
                        if !block
                            .get("input")
                            .and_then(Value::as_object)
                            .is_some_and(Map::is_empty)
                        {
                            return Err(IrError::InvalidJsonArguments);
                        }
                        let alias = string(block, "name")?;
                        let (tool, kind) = self.prepared.tools.resolve(alias)?;
                        self.prepared
                            .tools
                            .validate_count(self.tool_count() + 1, false)?;
                        let call = CallId::new(string(block, "id")?)?;
                        let item = tool_output(
                            &ToolCall {
                                status: Some(ToolCallStatus::InProgress),
                                item_id: Some(id.clone()),
                                call_id: call.clone(),
                                tool: tool.clone(),
                                input: if kind == ToolKind::Custom {
                                    ToolInput::Freeform(String::new())
                                } else {
                                    ToolInput::Json(String::new())
                                },
                                extensions: Extensions::responses(),
                            },
                            "in_progress",
                        );
                        (
                            BlockKind::Tool {
                                alias: alias.into(),
                                call: call.clone(),
                                kind,
                                raw: String::new(),
                            },
                            item,
                            OutputKind::Tool {
                                tool: tool.clone(),
                                call_id: call,
                                kind,
                            },
                        )
                    }
                    _ => return Err(unsupported()),
                };
                self.validator.apply(EventIR::ItemStarted {
                    id: id.clone(),
                    index: OutputIndex(index as u32),
                    kind: event_kind,
                })?;
                let is_text = matches!(block_kind, BlockKind::Text);
                self.blocks.push(Block {
                    id: id.clone(),
                    open: true,
                    kind: block_kind,
                    output: item.clone(),
                });
                self.emit(
                    "response.output_item.added",
                    json!({"output_index":index,"item":item}),
                    &mut output,
                );
                if is_text {
                    self.validator.apply(EventIR::PartStarted {
                        item: id.clone(),
                        index: ContentIndex(0),
                        kind: EventPartKind::Text,
                    })?;
                    let part = json!({"type":"output_text","text":"","annotations":[]});
                    self.blocks[index].output["content"] = json!([part]);
                    self.emit("response.content_part.added", json!({"item_id":id.as_str(),"output_index":index,"content_index":0,"part":part}), &mut output);
                    self.text_delta(index, string(block, "text")?, &mut output)?;
                }
            }
            "content_block_delta" => {
                known_fields(&value, &["type", "index", "delta"])?;
                if self.stop.is_some() {
                    return Err(IrError::InvalidEventOrder);
                }
                let index = index(&value)?;
                let block = self
                    .blocks
                    .get(index)
                    .filter(|b| b.open)
                    .ok_or(IrError::InvalidEventOrder)?;
                let delta = value.get("delta").ok_or(IrError::InvalidField("delta"))?;
                match (&block.kind, string(delta, "type")?) {
                    (BlockKind::Text, "text_delta") => {
                        known_fields(delta, &["type", "text"])?;
                        self.text_delta(index, string(delta, "text")?, &mut output)?;
                    }
                    (BlockKind::Tool { .. }, "input_json_delta") => {
                        known_fields(delta, &["type", "partial_json"])?;
                        let text = string(delta, "partial_json")?;
                        self.reserve(text.len())?;
                        let block = &mut self.blocks[index];
                        let BlockKind::Tool { kind, raw, .. } = &mut block.kind else {
                            unreachable!()
                        };
                        if text.len() > EventLimits::default().max_delta_bytes
                            || raw.len().saturating_add(text.len())
                                > EventLimits::default().max_argument_bytes
                        {
                            return Err(IrError::SizeLimit);
                        }
                        raw.push_str(text);
                        // Envelope bytes are private adapter state. Release freeform input only after validation.
                        if *kind == ToolKind::Function {
                            self.validator.apply(EventIR::ArgumentsDelta {
                                item: block.id.clone(),
                                text: text.into(),
                            })?;
                            let id = block.id.as_str().to_owned();
                            self.emit(
                                "response.function_call_arguments.delta",
                                json!({"item_id":id,"output_index":index,"delta":text}),
                                &mut output,
                            );
                        }
                    }
                    _ => return Err(unsupported()),
                }
            }
            "content_block_stop" => {
                known_fields(&value, &["type", "index"])?;
                if self.stop.is_some() {
                    return Err(IrError::InvalidEventOrder);
                }
                let index = index(&value)?;
                let block = self
                    .blocks
                    .get_mut(index)
                    .filter(|b| b.open)
                    .ok_or(IrError::InvalidEventOrder)?;
                let id = block.id.clone();
                match &mut block.kind {
                    BlockKind::Text => {
                        self.validator.apply(EventIR::PartFinished {
                            item: id.clone(),
                            index: ContentIndex(0),
                        })?;
                        let part = block.output["content"][0].clone();
                        self.emit("response.output_text.done", json!({"item_id":id.as_str(),"output_index":index,"content_index":0,"text":part["text"],"logprobs":[]}), &mut output);
                        self.emit("response.content_part.done", json!({"item_id":id.as_str(),"output_index":index,"content_index":0,"part":part}), &mut output);
                    }
                    BlockKind::Tool {
                        alias,
                        call,
                        kind,
                        raw,
                    } => {
                        let empty = raw.is_empty();
                        if empty {
                            raw.push_str("{}");
                        }
                        let restored = self.prepared.tools.restore(
                            alias,
                            call.clone(),
                            id.clone(),
                            raw.clone(),
                        )?;
                        let (text, event_prefix) = match &restored.input {
                            ToolInput::Json(v) => (v, "response.function_call_arguments"),
                            ToolInput::Freeform(v) => (v, "response.custom_tool_call_input"),
                        };
                        let emit_delta = *kind == ToolKind::Custom || empty;
                        if emit_delta {
                            // Bound emitted chunks even when a completed custom envelope is large.
                            for chunk in event_text_chunks(text) {
                                self.validator.apply(EventIR::ArgumentsDelta {
                                    item: id.clone(),
                                    text: chunk.into(),
                                })?;
                            }
                        }
                        block.output = tool_output(&restored, "completed");
                        if emit_delta {
                            for chunk in event_text_chunks(text) {
                                self.emit(&format!("{event_prefix}.delta"), json!({"item_id":id.as_str(),"output_index":index,"delta":chunk}), &mut output);
                            }
                        }
                        let mut done = json!({"item_id":id.as_str(),"output_index":index});
                        done[if matches!(restored.input, ToolInput::Freeform(_)) {
                            "input"
                        } else {
                            "arguments"
                        }] = json!(text);
                        self.emit(&format!("{event_prefix}.done"), done, &mut output);
                    }
                }
                self.validator.apply(EventIR::ItemFinished { item: id })?;
                self.blocks[index].open = false;
                self.blocks[index].output["status"] = json!("completed");
                self.emit(
                    "response.output_item.done",
                    json!({"output_index":index,"item":self.blocks[index].output}),
                    &mut output,
                );
            }
            "message_delta" => {
                known_fields(&value, &["type", "delta", "usage"])?;
                if self.stop.is_some() || self.blocks.iter().any(|b| b.open) {
                    return Err(IrError::InvalidEventOrder);
                }
                let delta = value.get("delta").ok_or(IrError::InvalidField("delta"))?;
                known_fields(
                    delta,
                    &["stop_reason", "stop_sequence", "stop_details", "container"],
                )?;
                for field in ["stop_details", "container"] {
                    if delta.get(field).is_some_and(|v| !v.is_null()) {
                        return Err(unsupported());
                    }
                }
                let stop = string(delta, "stop_reason")?;
                self.prepared.terminal(stop, self.tool_count())?;
                let update = value.get("usage").ok_or(IrError::InvalidField("usage"))?;
                // Provider token counters are cumulative; retain fields absent from the delta.
                let mut merged = self.usage.clone();
                for (key, value) in object(update)? {
                    merged[key] = value.clone();
                }
                let (input, generated, _) = usage(&merged)?;
                self.validator.apply(EventIR::UsageUpdated(Usage {
                    input_tokens: Some(input),
                    output_tokens: Some(generated),
                }))?;
                self.usage = merged;
                self.stop = Some(stop.into());
            }
            "message_stop" => {
                known_fields(&value, &["type"])?;
                let stop = self.stop.as_ref().ok_or(IrError::InvalidEventOrder)?;
                let (terminal, reason) = self.prepared.terminal(stop, self.tool_count())?;
                self.validator.apply(EventIR::Finished {
                    status: terminal,
                    reason: reason.map(str::to_owned),
                })?;
                let status = if terminal == Terminal::Completed {
                    "completed"
                } else {
                    "incomplete"
                };
                let (input, generated, total) = usage(&self.usage)?;
                let response = self.response.as_mut().expect("started");
                let mut items: Vec<Value> = self.blocks.iter().map(|b| b.output.clone()).collect();
                if terminal == Terminal::Incomplete {
                    for item in &mut items {
                        item["status"] = json!("incomplete");
                    }
                }
                response["status"] = json!(status);
                response["output"] = json!(items);
                response["usage"] =
                    json!({"input_tokens":input,"output_tokens":generated,"total_tokens":total});
                if let Some(reason) = reason {
                    response["incomplete_details"] = json!({"reason":reason});
                }
                let response = response.clone();
                self.emit(
                    &format!("response.{status}"),
                    json!({"response":response}),
                    &mut output,
                );
                self.done = true;
            }
            _ => return Err(unsupported()),
        }
        Ok(output)
    }
    fn text_delta(
        &mut self,
        index: usize,
        text: &str,
        output: &mut Vec<Value>,
    ) -> Result<(), IrError> {
        self.reserve(text.len())?;
        let block = &mut self.blocks[index];
        self.validator.apply(EventIR::TextDelta {
            item: block.id.clone(),
            index: ContentIndex(0),
            text: text.into(),
        })?;
        let Value::String(accumulated) = &mut block.output["content"][0]["text"] else {
            unreachable!("constructed text")
        };
        accumulated.push_str(text);
        let id = block.id.as_str().to_owned();
        if !text.is_empty() {
            self.emit("response.output_text.delta", json!({"item_id":id,"output_index":index,"content_index":0,"delta":text,"logprobs":[]}), output);
        }
        Ok(())
    }
    fn tool_count(&self) -> usize {
        self.blocks
            .iter()
            .filter(|b| matches!(b.kind, BlockKind::Tool { .. }))
            .count()
    }
}
fn index(value: &Value) -> Result<usize, IrError> {
    value
        .get("index")
        .and_then(Value::as_u64)
        .and_then(|v| usize::try_from(v).ok())
        .ok_or(IrError::InvalidField("index"))
}
