//! Validate public managed progress independently from native replay and codec state.
use crate::{
    adapters::json::{known_fields, string},
    ir::IrError,
};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Progress {
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
    pub(super) fn observe(&mut self, events: &[Value], maximum: usize) -> Result<(), IrError> {
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
    pub(super) fn finish(&self, response: &Value) -> Result<(), IrError> {
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
