//! Text progresses immediately. Executable tool completions wait for a validated terminal.
use super::*;
use crate::adapters::sse::SseEvent;

struct Part {
    value: Value,
    text_done: bool,
    closed: bool,
}

struct StreamItem {
    head: Value,
    parts: Vec<Part>,
    arguments: String,
    arguments_done: bool,
    done: Option<Value>,
}

pub struct ResponsesStream<'a> {
    prepared: &'a PreparedResponses,
    validator: EventValidator,
    response: Option<(String, u64)>,
    items: BTreeMap<usize, StreamItem>,
    item_ids: BTreeSet<String>,
    call_ids: BTreeSet<String>,
    counters: gateway_usage_contract::Accumulator,
    sequence: u64,
    source_sequence: Option<u64>,
    limit: usize,
    buffered: usize,
    progress_seen: bool,
    complete: bool,
    invalid: bool,
}

impl<'a> ResponsesStream<'a> {
    pub(super) fn new(prepared: &'a PreparedResponses, limit: usize) -> Result<Self, IrError> {
        if limit == 0 {
            return Err(IrError::SizeLimit);
        }
        Ok(Self {
            prepared,
            validator: EventValidator::new(EventLimits::default())?,
            response: None,
            items: BTreeMap::new(),
            item_ids: BTreeSet::new(),
            call_ids: BTreeSet::new(),
            counters: gateway_usage_contract::Accumulator::new(
                gateway_usage_contract::Profile::ResponsesV1,
            ),
            sequence: 0,
            source_sequence: None,
            limit,
            buffered: 0,
            progress_seen: false,
            complete: false,
            invalid: false,
        })
    }

    pub fn event(&mut self, event: SseEvent) -> Result<Vec<Value>, IrError> {
        if self.invalid || self.complete {
            return Err(IrError::InvalidEventOrder);
        }
        let result = self.convert(event);
        if result.is_err() {
            self.invalid = true;
        }
        result
    }

    pub fn finish(&self) -> Result<(), IrError> {
        if self.complete && !self.invalid {
            Ok(())
        } else {
            Err(IrError::InvalidEventOrder)
        }
    }
    pub fn is_complete(&self) -> bool {
        self.complete && !self.invalid
    }

    fn reserve(&mut self, bytes: usize) -> Result<(), IrError> {
        if bytes > self.limit.saturating_sub(self.buffered) {
            return Err(IrError::SizeLimit);
        }
        self.buffered += bytes;
        Ok(())
    }
    fn emit(&mut self, mut event: Value, output: &mut Vec<Value>) -> Result<(), IrError> {
        event["sequence_number"] = json!(self.sequence);
        self.sequence = self.sequence.checked_add(1).ok_or(IrError::SizeLimit)?;
        if serde_json::to_vec(&event).expect("event").len() > self.limit {
            return Err(IrError::SizeLimit);
        }
        output.push(event);
        Ok(())
    }
    fn check_response(&self, value: &Value) -> Result<(), IrError> {
        self.prepared.header(value)?;
        if let Some((id, created)) = &self.response
            && (value["id"] != *id || value["created_at"] != *created)
        {
            return Err(IrError::InvalidEventOrder);
        }
        Ok(())
    }

    fn convert(&mut self, event: SseEvent) -> Result<Vec<Value>, IrError> {
        if event.data.len() > self.limit {
            return Err(IrError::SizeLimit);
        }
        let value = decode(event.data.as_bytes())?;
        let kind = string(&value, "type")?;
        if event.event != kind {
            return Err(IrError::InvalidField("responses_event"));
        }
        let sequence = value["sequence_number"]
            .as_u64()
            .ok_or(IrError::InvalidField("sequence_number"))?;
        if self.source_sequence.is_some_and(|old| sequence <= old) {
            return Err(IrError::InvalidEventOrder);
        }
        self.source_sequence = Some(sequence);
        if let Some(counters) = value.pointer("/response/usage").filter(|v| !v.is_null()) {
            self.counters.observe(counters);
        }
        if self
            .counters
            .usage
            .violations
            .iter()
            .any(|v| matches!(v.as_str(), "invalid_counter" | "counter_decreased"))
        {
            return Err(IrError::InvalidField("usage"));
        }
        let mut output = Vec::new();
        match kind {
            "response.created" => {
                known_fields(&value, &["type", "sequence_number", "response"])?;
                if self.response.is_some() {
                    return Err(IrError::InvalidEventOrder);
                }
                let mut response = value["response"].clone();
                self.check_response(&response)?;
                if response["status"] != "in_progress" || response["output"] != json!([]) {
                    return Err(IrError::InvalidEventOrder);
                }
                let id = string(&response, "id")?.to_owned();
                self.validator.apply(EventIR::Started {
                    id: ResponseId::new(&id)?,
                })?;
                self.response = Some((id, response["created_at"].as_u64().expect("checked")));
                self.prepared.restore_echoes(&mut response)?;
                self.emit(json!({"type":kind,"response":response}), &mut output)?;
            }
            _ if self.response.is_none() => return Err(IrError::InvalidEventOrder),
            "response.in_progress" => {
                known_fields(&value, &["type", "sequence_number", "response"])?;
                self.check_response(&value["response"])?;
                if self.progress_seen
                    || !self.items.is_empty()
                    || value["response"]["status"] != "in_progress"
                    || value["response"]["output"] != json!([])
                {
                    return Err(IrError::InvalidEventOrder);
                }
                self.progress_seen = true;
                let mut value = value;
                self.prepared.restore_echoes(&mut value["response"])?;
                self.emit(value, &mut output)?;
            }
            "response.output_item.added" => {
                known_fields(&value, &["type", "sequence_number", "output_index", "item"])?;
                let index = index(&value, "output_index")?;
                let item = &value["item"];
                let id = string(item, "id")?;
                ItemId::new(id)?;
                if self.items.len() >= EventLimits::default().max_items
                    || self.items.contains_key(&index)
                    || !self.item_ids.insert(id.into())
                {
                    return Err(IrError::DuplicateId);
                }
                let item_kind = string(item, "type")?;
                let mut parts = Vec::new();
                match item_kind {
                    "function_call" | "custom_tool_call" => {
                        known_fields(
                            item,
                            &[
                                "id",
                                "type",
                                "call_id",
                                "name",
                                "namespace",
                                "status",
                                "input",
                                "arguments",
                            ],
                        )?;
                        let custom = item_kind == "custom_tool_call";
                        let input = if custom { "input" } else { "arguments" };
                        if item
                            .get(if custom { "arguments" } else { "input" })
                            .is_some()
                            || !string(item, input)?.is_empty()
                            || item["status"] != "in_progress"
                        {
                            return Err(IrError::InvalidToolMapping);
                        }
                        let identity = identity(item)?;
                        self.prepared.tools.resolve_identity(&identity)?;
                        // The selected alias must also have the expected wire input kind.
                        let call_id = CallId::new(string(item, "call_id")?)?;
                        let definition = self
                            .prepared
                            .tools
                            .registry
                            .definitions()
                            .iter()
                            .flat_map(|t| match &t.kind {
                                crate::ir::request::ToolDefinitionKind::Namespace { tools } => {
                                    tools.as_slice()
                                }
                                _ => std::slice::from_ref(t),
                            })
                            .find(|t| t.identity == identity)
                            .ok_or(IrError::InvalidToolMapping)?;
                        if definition.kind()
                            != Some(if custom {
                                crate::ir::ToolKind::Custom
                            } else {
                                crate::ir::ToolKind::Function
                            })
                        {
                            return Err(IrError::InvalidToolMapping);
                        }
                        if !self.call_ids.insert(call_id.as_str().into()) {
                            return Err(IrError::DuplicateId);
                        }
                        self.prepared
                            .tools
                            .validate_count(self.call_ids.len(), false)?;
                    }
                    "message" | "reasoning" => {
                        let reasoning = item_kind == "reasoning";
                        known_fields(
                            item,
                            if reasoning {
                                &["id", "type", "summary", "status", "encrypted_content"]
                            } else {
                                &["id", "type", "role", "content", "status", "phase"]
                            },
                        )?;
                        if item.get("encrypted_content").is_some_and(|v| !v.is_null())
                            || item.get("phase").is_some_and(|v| !valid_phase(v))
                        {
                            return Err(IrError::UnsupportedFeature);
                        }
                        let field = if reasoning { "summary" } else { "content" };
                        if item[field] != json!([])
                            || (!reasoning && item["role"] != "assistant")
                            || item.get("status").is_some_and(|v| v != "in_progress")
                        {
                            return Err(IrError::InvalidEventOrder);
                        }
                        if reasoning
                            && self.prepared.profile.support(Feature::ReasoningItems)
                                != Support::Native
                        {
                            return Err(IrError::UnsupportedFeature);
                        }
                        self.validator.apply(EventIR::ItemStarted {
                            id: ItemId::new(id)?,
                            index: OutputIndex(
                                u32::try_from(index).map_err(|_| IrError::SizeLimit)?,
                            ),
                            kind: if reasoning {
                                OutputKind::Reasoning
                            } else {
                                OutputKind::Message
                            },
                        })?;
                        parts = Vec::new();
                        self.emit(value.clone(), &mut output)?;
                    }
                    _ => return Err(IrError::UnsupportedFeature),
                }
                self.reserve(serde_json::to_vec(item).expect("item").len())?;
                self.items.insert(
                    index,
                    StreamItem {
                        head: item.clone(),
                        parts,
                        arguments: String::new(),
                        arguments_done: false,
                        done: None,
                    },
                );
            }
            "response.function_call_arguments.delta" | "response.custom_tool_call_input.delta" => {
                known_fields(
                    &value,
                    &[
                        "type",
                        "sequence_number",
                        "item_id",
                        "output_index",
                        "delta",
                    ],
                )?;
                let text = string(&value, "delta")?;
                if text.len() > EventLimits::default().max_delta_bytes {
                    return Err(IrError::SizeLimit);
                }
                self.reserve(text.len())?;
                let item = self.item_mut(&value)?;
                check_tool_event(item, kind)?;
                if item.arguments_done {
                    return Err(IrError::InvalidEventOrder);
                }
                if text.len()
                    > EventLimits::default()
                        .max_argument_bytes
                        .saturating_sub(item.arguments.len())
                {
                    return Err(IrError::SizeLimit);
                }
                item.arguments.push_str(text);
            }
            "response.function_call_arguments.done" | "response.custom_tool_call_input.done" => {
                let field = if kind.contains("custom_tool") {
                    "input"
                } else {
                    "arguments"
                };
                known_fields(
                    &value,
                    &["type", "sequence_number", "item_id", "output_index", field],
                )?;
                let item = self.item_mut(&value)?;
                check_tool_event(item, kind)?;
                if item.arguments_done || string(&value, field)? != item.arguments {
                    return Err(IrError::InvalidEventOrder);
                }
                item.arguments_done = true;
            }
            "response.content_part.added" | "response.reasoning_summary_part.added" => {
                let reasoning = kind.contains("reasoning");
                let key = if reasoning {
                    "summary_index"
                } else {
                    "content_index"
                };
                known_fields(
                    &value,
                    &[
                        "type",
                        "sequence_number",
                        "item_id",
                        "output_index",
                        key,
                        "part",
                    ],
                )?;
                let part = &value["part"];
                validate_part(part, reasoning)?;
                if part["text"] != "" {
                    return Err(IrError::InvalidEventOrder);
                }
                let n = index(&value, key)?;
                let item = self.item_mut(&value)?;
                if item.head["type"] != if reasoning { "reasoning" } else { "message" }
                    || n != item.parts.len()
                {
                    return Err(IrError::InvalidEventOrder);
                }
                item.parts.push(Part {
                    value: part.clone(),
                    text_done: false,
                    closed: false,
                });
                self.validator.apply(EventIR::PartStarted {
                    item: ItemId::new(string(&value, "item_id")?)?,
                    index: ContentIndex(u32::try_from(n).map_err(|_| IrError::SizeLimit)?),
                    kind: if reasoning {
                        PartKind::ReasoningText
                    } else {
                        PartKind::Text
                    },
                })?;
                self.emit(value, &mut output)?;
            }
            "response.output_text.delta" | "response.reasoning_summary_text.delta" => {
                let reasoning = kind.contains("reasoning");
                let key = if reasoning {
                    "summary_index"
                } else {
                    "content_index"
                };
                known_fields(
                    &value,
                    &[
                        "type",
                        "sequence_number",
                        "item_id",
                        "output_index",
                        key,
                        "delta",
                        "logprobs",
                    ],
                )?;
                empty_logprobs(&value)?;
                let text = string(&value, "delta")?;
                self.reserve(text.len())?;
                let n = index(&value, key)?;
                let part = self.part_mut(&value, reasoning, n)?;
                if part.text_done {
                    return Err(IrError::InvalidEventOrder);
                }
                let mut full = string(&part.value, "text")?.to_owned();
                full.push_str(text);
                part.value["text"] = json!(full);
                self.validator.apply(EventIR::TextDelta {
                    item: ItemId::new(string(&value, "item_id")?)?,
                    index: ContentIndex(u32::try_from(n).map_err(|_| IrError::SizeLimit)?),
                    text: text.into(),
                })?;
                self.emit(value, &mut output)?;
            }
            "response.output_text.done" | "response.reasoning_summary_text.done" => {
                let reasoning = kind.contains("reasoning");
                let key = if reasoning {
                    "summary_index"
                } else {
                    "content_index"
                };
                known_fields(
                    &value,
                    &[
                        "type",
                        "sequence_number",
                        "item_id",
                        "output_index",
                        key,
                        "text",
                        "logprobs",
                    ],
                )?;
                empty_logprobs(&value)?;
                let n = index(&value, key)?;
                let part = self.part_mut(&value, reasoning, n)?;
                if part.text_done || value["text"] != part.value["text"] {
                    return Err(IrError::InvalidEventOrder);
                }
                part.text_done = true;
                self.emit(value, &mut output)?;
            }
            "response.content_part.done" | "response.reasoning_summary_part.done" => {
                let reasoning = kind.contains("reasoning");
                let key = if reasoning {
                    "summary_index"
                } else {
                    "content_index"
                };
                known_fields(
                    &value,
                    &[
                        "type",
                        "sequence_number",
                        "item_id",
                        "output_index",
                        key,
                        "part",
                    ],
                )?;
                let n = index(&value, key)?;
                let part = self.part_mut(&value, reasoning, n)?;
                if !part.text_done || part.value != value["part"] {
                    return Err(IrError::InvalidEventOrder);
                }
                part.closed = true;
                self.validator.apply(EventIR::PartFinished {
                    item: ItemId::new(string(&value, "item_id")?)?,
                    index: ContentIndex(u32::try_from(n).map_err(|_| IrError::SizeLimit)?),
                })?;
                self.emit(value, &mut output)?;
            }
            "response.output_item.done" => {
                known_fields(&value, &["type", "sequence_number", "output_index", "item"])?;
                let n = index(&value, "output_index")?;
                let original = &value["item"];
                let restored = self.prepared.restore_item(original)?;
                let item = self
                    .items
                    .get_mut(&n)
                    .filter(|s| s.done.is_none())
                    .ok_or(IrError::InvalidEventOrder)?;
                for key in [
                    "id",
                    "type",
                    "role",
                    "call_id",
                    "name",
                    "namespace",
                    "phase",
                ] {
                    if item.head.get(key) != original.get(key) {
                        return Err(IrError::InvalidToolMapping);
                    }
                }
                if is_tool(original) {
                    let key = if original["type"] == "custom_tool_call" {
                        "input"
                    } else {
                        "arguments"
                    };
                    if !item.arguments_done || original[key] != item.arguments {
                        return Err(IrError::InvalidEventOrder);
                    }
                } else {
                    let key = if original["type"] == "reasoning" {
                        "summary"
                    } else {
                        "content"
                    };
                    let parts: Vec<_> = item.parts.iter().map(|p| p.value.clone()).collect();
                    if item.parts.iter().any(|p| !p.closed) || original[key] != json!(parts) {
                        return Err(IrError::InvalidEventOrder);
                    }
                    self.validator.apply(EventIR::ItemFinished {
                        item: ItemId::new(string(original, "id")?)?,
                    })?;
                }
                item.done = Some(original.clone());
                if !is_tool(&restored) {
                    self.emit(value, &mut output)?;
                }
            }
            "response.completed" | "response.incomplete" | "response.failed" => {
                known_fields(&value, &["type", "sequence_number", "response"])?;
                let response = &value["response"];
                self.check_response(response)?;
                let status = kind.strip_prefix("response.").expect("prefix");
                if response["status"] != status {
                    return Err(IrError::InvalidEventOrder);
                }
                let raw_items = response["output"]
                    .as_array()
                    .ok_or(IrError::InvalidField("output"))?;
                if raw_items.len() != self.items.len() || !self.counters.usage.violations.is_empty()
                {
                    return Err(IrError::InvalidEventOrder);
                }
                for (n, raw) in raw_items.iter().enumerate() {
                    let item = self.items.get(&n).ok_or(IrError::InvalidEventOrder)?;
                    if item.done.as_ref() != Some(raw) {
                        return Err(IrError::InvalidEventOrder);
                    }
                }
                let restored = self.prepared.decode_value(response.clone())?;
                for (n, item) in restored["output"]
                    .as_array()
                    .expect("output")
                    .iter()
                    .enumerate()
                {
                    if is_tool(item) {
                        validate_complete_item(&mut self.validator, item, n)?;
                        self.emit_tool(item, n, &mut output)?;
                    }
                }
                self.validator.apply(EventIR::Finished {
                    status: terminal(status)?,
                    reason: None,
                })?;
                self.emit(json!({"type":kind,"response":restored}), &mut output)?;
                self.complete = true;
            }
            _ => return Err(IrError::UnsupportedFeature),
        }
        Ok(output)
    }

    fn item_mut(&mut self, value: &Value) -> Result<&mut StreamItem, IrError> {
        let n = index(value, "output_index")?;
        let item = self
            .items
            .get_mut(&n)
            .filter(|s| s.done.is_none())
            .ok_or(IrError::InvalidEventOrder)?;
        if value["item_id"] != item.head["id"] {
            return Err(IrError::InvalidToolMapping);
        }
        Ok(item)
    }
    fn part_mut(&mut self, value: &Value, reasoning: bool, n: usize) -> Result<&mut Part, IrError> {
        let item = self.item_mut(value)?;
        if item.head["type"] != if reasoning { "reasoning" } else { "message" } {
            return Err(IrError::InvalidEventOrder);
        }
        item.parts
            .get_mut(n)
            .filter(|p| !p.closed)
            .ok_or(IrError::InvalidEventOrder)
    }
    fn emit_tool(
        &mut self,
        item: &Value,
        n: usize,
        output: &mut Vec<Value>,
    ) -> Result<(), IrError> {
        let custom = item["type"] == "custom_tool_call";
        let key = if custom { "input" } else { "arguments" };
        let prefix = if custom {
            "response.custom_tool_call_input"
        } else {
            "response.function_call_arguments"
        };
        let mut started = item.clone();
        started["status"] = json!("in_progress");
        started[key] = json!("");
        self.emit(
            json!({"type":"response.output_item.added","output_index":n,"item":started}),
            output,
        )?;
        for chunk in event_text_chunks(string(item, key)?) {
            self.emit(json!({"type":format!("{prefix}.delta"),"item_id":item["id"],"output_index":n,"delta":chunk}),output)?;
        }
        let mut done =
            json!({"type":format!("{prefix}.done"),"item_id":item["id"],"output_index":n});
        done[key] = item[key].clone();
        self.emit(done, output)?;
        self.emit(
            json!({"type":"response.output_item.done","output_index":n,"item":item}),
            output,
        )
    }
}

fn index(value: &Value, key: &'static str) -> Result<usize, IrError> {
    value[key]
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .filter(|n| *n < EventLimits::default().max_parts)
        .ok_or(IrError::InvalidField(key))
}
fn check_tool_event(item: &StreamItem, kind: &str) -> Result<(), IrError> {
    if item.head["type"]
        != if kind.contains("custom_tool") {
            "custom_tool_call"
        } else {
            "function_call"
        }
    {
        return Err(IrError::InvalidToolMapping);
    }
    Ok(())
}
fn empty_logprobs(value: &Value) -> Result<(), IrError> {
    if value
        .get("logprobs")
        .is_some_and(|v| !v.is_null() && !v.as_array().is_some_and(Vec::is_empty))
    {
        Err(IrError::UnsupportedFeature)
    } else {
        Ok(())
    }
}
