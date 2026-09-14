//! Validate public managed progress independently from native replay and codec state.
use crate::{
    adapters::json::{known_fields, string},
    ir::IrError,
};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct Progress {
    items: BTreeMap<String, Item>,
    bytes: usize,
    created: Option<String>,
}
struct Item {
    kind: String,
    index: u64,
    parts: BTreeMap<u64, String>,
}
impl Progress {
    pub(crate) fn observe(&mut self, events: &[Value], maximum: usize) -> Result<(), IrError> {
        for event in events {
            self.bytes = self
                .bytes
                .checked_add(event.to_string().len())
                .ok_or(IrError::SizeLimit)?;
            if self.bytes > maximum {
                return Err(IrError::SizeLimit);
            }
            match string(event, "type")? {
                "response.created" => {
                    known_fields(event, &["type", "response"])?;
                    if self.created.is_some() || !self.items.is_empty() {
                        return Err(IrError::InvalidEventOrder);
                    }
                    self.created = Some(string(&event["response"], "id")?.into());
                    if event["response"]["output"]
                        .as_array()
                        .is_some_and(|a| !a.is_empty())
                    {
                        return Err(IrError::InvalidEventOrder);
                    }
                }
                "response.output_item.added" => {
                    known_fields(event, &["type", "output_index", "item"])?;
                    let item = &event["item"];
                    let kind = string(item, "type")?;
                    if !matches!(kind, "message" | "reasoning") {
                        return Err(IrError::InvalidEventOrder);
                    }
                    known_fields(
                        item,
                        if kind == "message" {
                            &["id", "type", "role", "status", "content"]
                        } else {
                            &["id", "type", "summary"]
                        },
                    )?;
                    if (kind == "message"
                        && (item["role"] != "assistant" || item["status"] != "in_progress"))
                        || !item[if kind == "message" {
                            "content"
                        } else {
                            "summary"
                        }]
                        .as_array()
                        .is_some_and(Vec::is_empty)
                    {
                        return Err(IrError::InvalidEventOrder);
                    }
                    let index = event["output_index"]
                        .as_u64()
                        .ok_or(IrError::InvalidEventOrder)?;
                    if index >= 4096
                        || self.items.values().any(|i| i.index >= index)
                        || self.items.len() >= 4096
                    {
                        return Err(IrError::InvalidEventOrder);
                    }
                    if self
                        .items
                        .insert(
                            string(item, "id")?.into(),
                            Item {
                                kind: kind.into(),
                                index,
                                parts: BTreeMap::new(),
                            },
                        )
                        .is_some()
                    {
                        return Err(IrError::DuplicateId);
                    }
                }
                kind @ ("response.content_part.added"
                | "response.reasoning_summary_part.added"
                | "response.output_text.delta"
                | "response.reasoning_summary_text.delta") => {
                    let reasoning = kind.contains("reasoning");
                    let index_key = if reasoning {
                        "summary_index"
                    } else {
                        "content_index"
                    };
                    let added = kind.ends_with("added");
                    known_fields(
                        event,
                        &[
                            "type",
                            "output_index",
                            "item_id",
                            index_key,
                            if added { "part" } else { "delta" },
                        ],
                    )?;
                    let item = self
                        .items
                        .get_mut(string(event, "item_id")?)
                        .ok_or(IrError::UnknownItem)?;
                    if event["output_index"].as_u64() != Some(item.index)
                        || (item.kind == "reasoning") != reasoning
                    {
                        return Err(IrError::InvalidEventOrder);
                    }
                    let index = event[index_key]
                        .as_u64()
                        .ok_or(IrError::InvalidEventOrder)?;
                    if added {
                        let part = &event["part"];
                        known_fields(part, &["type", "text", "annotations"])?;
                        if part["type"]
                            != if reasoning {
                                "summary_text"
                            } else {
                                "output_text"
                            }
                            || part["text"] != ""
                            || part
                                .get("annotations")
                                .is_some_and(|v| v != &serde_json::json!([]))
                            || index != item.parts.len() as u64
                            || item.parts.len() >= 16384
                        {
                            return Err(IrError::InvalidEventOrder);
                        }
                        item.parts.insert(index, String::new());
                    } else {
                        item.parts
                            .get_mut(&index)
                            .ok_or(IrError::InvalidEventOrder)?
                            .push_str(string(event, "delta")?);
                    }
                }
                _ => return Err(IrError::InvalidEventOrder),
            }
        }
        Ok(())
    }
    pub(crate) fn finish(&self, response: &Value) -> Result<(), IrError> {
        if self
            .created
            .as_ref()
            .is_some_and(|id| response["id"] != *id)
        {
            return Err(IrError::InvalidEventOrder);
        }
        let output = response["output"]
            .as_array()
            .ok_or(IrError::InvalidEventOrder)?;
        for (id, item) in &self.items {
            let final_item = output
                .iter()
                .find(|v| v["id"] == *id)
                .ok_or(IrError::UnknownItem)?;
            if final_item["type"] != item.kind {
                return Err(IrError::InvalidEventOrder);
            }
            let parts = final_item[if item.kind == "reasoning" {
                "summary"
            } else {
                "content"
            }]
            .as_array()
            .ok_or(IrError::InvalidEventOrder)?;
            for (index, text) in &item.parts {
                if parts.get(*index as usize).and_then(|v| v["text"].as_str()) != Some(text) {
                    return Err(IrError::InvalidEventOrder);
                }
            }
        }
        Ok(())
    }
}

impl Progress {
    /// The new role requires complete public ordering before releasing progress.
    pub(crate) fn observe_provider(
        &mut self,
        events: &[Value],
        maximum: usize,
    ) -> Result<(), IrError> {
        for event in events {
            if event["type"] != "response.created" && self.created.is_none() {
                return Err(IrError::InvalidEventOrder);
            }
            if event["type"] == "response.output_item.added"
                && event["output_index"].as_u64() != Some(self.items.len() as u64)
            {
                return Err(IrError::InvalidEventOrder);
            }
            self.observe(std::slice::from_ref(event), maximum)?;
        }
        Ok(())
    }
    /// Host-authored terminal events for the provider role's nonterminal subset.
    /// Existing codec behavior continues to use observe/finish without this builder.
    pub(crate) fn terminal_events(
        &self,
        response: &Value,
        maximum: usize,
    ) -> Result<Vec<Value>, IrError> {
        use serde_json::json;
        self.finish(response)?;
        let mut events = Vec::new();
        if self.created.is_none() {
            if !self.items.is_empty() {
                return Err(IrError::InvalidEventOrder);
            }
            let mut created = response.clone();
            created["status"] = json!("in_progress");
            created["output"] = json!([]);
            created
                .as_object_mut()
                .ok_or(IrError::InvalidEventOrder)?
                .remove("output_text");
            created.as_object_mut().unwrap().remove("usage");
            events.push(json!({"type":"response.created","response":created}));
        }
        for (index, item) in response["output"]
            .as_array()
            .ok_or(IrError::InvalidEventOrder)?
            .iter()
            .enumerate()
        {
            let id = string(item, "id")?;
            let kind = string(item, "type")?;
            let observed = self.items.get(id);
            if observed.is_some_and(|item| item.index != index as u64) {
                return Err(IrError::InvalidEventOrder);
            }
            let tool = matches!(kind, "function_call" | "custom_tool_call");
            let reasoning = kind == "reasoning";
            let key = if kind == "custom_tool_call" {
                "input"
            } else {
                "arguments"
            };
            if observed.is_none() {
                // An earlier unobserved item would reorder already released progress.
                if self.items.values().any(|item| item.index > index as u64) {
                    return Err(IrError::InvalidEventOrder);
                }
                let mut started = item.clone();
                if tool {
                    started["status"] = json!("in_progress");
                    started[key] = json!("");
                } else if reasoning {
                    started["summary"] = json!([]);
                } else {
                    started["status"] = json!("in_progress");
                    started["content"] = json!([]);
                }
                events.push(json!({"type":"response.output_item.added","output_index":index,"item":started}));
            }
            if tool {
                let prefix = if kind == "custom_tool_call" {
                    "response.custom_tool_call_input"
                } else {
                    "response.function_call_arguments"
                };
                for chunk in crate::adapters::json::event_text_chunks(string(item, key)?) {
                    events.push(json!({"type":format!("{prefix}.delta"),"item_id":id,"output_index":index,"delta":chunk}));
                }
                let mut done =
                    json!({"type":format!("{prefix}.done"),"item_id":id,"output_index":index});
                done[key] = item[key].clone();
                events.push(done);
            } else {
                let index_key = if reasoning {
                    "summary_index"
                } else {
                    "content_index"
                };
                let part_prefix = if reasoning {
                    "response.reasoning_summary_part"
                } else {
                    "response.content_part"
                };
                let text_prefix = if reasoning {
                    "response.reasoning_summary_text"
                } else {
                    "response.output_text"
                };
                for (part_index, part) in item[if reasoning { "summary" } else { "content" }]
                    .as_array()
                    .ok_or(IrError::InvalidEventOrder)?
                    .iter()
                    .enumerate()
                {
                    if observed.is_none_or(|item| !item.parts.contains_key(&(part_index as u64))) {
                        let mut empty = part.clone();
                        empty["text"] = json!("");
                        let mut added = json!({"type":format!("{part_prefix}.added"),"item_id":id,"output_index":index,"part":empty});
                        added[index_key] = json!(part_index);
                        events.push(added);
                        for chunk in crate::adapters::json::event_text_chunks(string(part, "text")?)
                        {
                            let mut delta = json!({"type":format!("{text_prefix}.delta"),"item_id":id,"output_index":index,"delta":chunk});
                            delta[index_key] = json!(part_index);
                            events.push(delta);
                        }
                    }
                    let mut text_done = json!({"type":format!("{text_prefix}.done"),"item_id":id,"output_index":index,"text":part["text"]});
                    text_done[index_key] = json!(part_index);
                    events.push(text_done);
                    let mut part_done = json!({"type":format!("{part_prefix}.done"),"item_id":id,"output_index":index,"part":part});
                    part_done[index_key] = json!(part_index);
                    events.push(part_done);
                }
            }
            events
                .push(json!({"type":"response.output_item.done","output_index":index,"item":item}));
        }
        let terminal = match string(response, "status")? {
            "completed" => "response.completed",
            "incomplete" => "response.incomplete",
            _ => return Err(IrError::InvalidEventOrder),
        };
        events.push(json!({"type":terminal,"response":response}));
        let bytes = serde_json::to_vec(&events)
            .map_err(|_| IrError::InvalidEventOrder)?
            .len();
        if bytes > maximum {
            return Err(IrError::SizeLimit);
        }
        Ok(events)
    }
}
