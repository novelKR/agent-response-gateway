use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::{CallId, IrError, ItemId, ResponseId, ToolIdentity, ToolKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct OutputIndex(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ContentIndex(pub u32);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutputKind {
    Message,
    Reasoning,
    Tool {
        tool: ToolIdentity,
        call_id: CallId,
        kind: ToolKind,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartKind {
    Text,
    ReasoningText,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Terminal {
    Completed,
    Incomplete,
    Failed,
    Cancelled,
    TransportLost,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

/// Deltas carry strings only after wire UTF-8 decoding; no SSE dialect is defined here.
#[derive(Clone, PartialEq, Eq)]
pub enum EventIR {
    Started {
        id: ResponseId,
    },
    ItemStarted {
        id: ItemId,
        index: OutputIndex,
        kind: OutputKind,
    },
    PartStarted {
        item: ItemId,
        index: ContentIndex,
        kind: PartKind,
    },
    TextDelta {
        item: ItemId,
        index: ContentIndex,
        text: String,
    },
    PartFinished {
        item: ItemId,
        index: ContentIndex,
    },
    ArgumentsDelta {
        item: ItemId,
        text: String,
    },
    ItemFinished {
        item: ItemId,
    },
    UsageUpdated(Usage),
    Finished {
        status: Terminal,
        reason: Option<String>,
    },
}

struct PartState {
    open: bool,
}
struct ItemState {
    kind: OutputKind,
    open: bool,
    parts: BTreeMap<ContentIndex, PartState>,
    arguments: String,
}

#[derive(Clone, Copy, Debug)]
pub struct EventLimits {
    pub max_items: usize,
    pub max_parts: usize,
    pub max_delta_bytes: usize,
    pub max_argument_bytes: usize,
}
impl Default for EventLimits {
    fn default() -> Self {
        Self {
            max_items: 4096,
            max_parts: 16384,
            max_delta_bytes: 1024 * 1024,
            max_argument_bytes: 8 * 1024 * 1024,
        }
    }
}

/// Pure validation/reduction. A rejected event leaves the prior state unchanged.
pub struct EventValidator {
    started: bool,
    terminal: Option<Terminal>,
    terminal_reason: Option<String>,
    items: BTreeMap<ItemId, ItemState>,
    indices: BTreeSet<OutputIndex>,
    calls: BTreeSet<CallId>,
    parts: usize,
    argument_bytes: usize,
    usage: Option<Usage>,
    limits: EventLimits,
}

impl EventValidator {
    pub fn new(limits: EventLimits) -> Result<Self, IrError> {
        if limits.max_items == 0
            || limits.max_parts == 0
            || limits.max_delta_bytes == 0
            || limits.max_argument_bytes == 0
        {
            return Err(IrError::SizeLimit);
        }
        Ok(Self {
            started: false,
            terminal: None,
            terminal_reason: None,
            items: BTreeMap::new(),
            indices: BTreeSet::new(),
            calls: BTreeSet::new(),
            parts: 0,
            argument_bytes: 0,
            usage: None,
            limits,
        })
    }
    pub fn terminal(&self) -> Option<Terminal> {
        self.terminal
    }
    pub fn terminal_reason(&self) -> Option<&str> {
        self.terminal_reason.as_deref()
    }
    pub fn usage(&self) -> Option<&Usage> {
        self.usage.as_ref()
    }
    pub fn arguments(&self, item: &ItemId) -> Result<&str, IrError> {
        let state = self.items.get(item).ok_or(IrError::UnknownItem)?;
        if state.open || !matches!(state.kind, OutputKind::Tool { .. }) {
            return Err(IrError::InvalidEventOrder);
        }
        Ok(&state.arguments)
    }

    pub fn apply(&mut self, event: EventIR) -> Result<(), IrError> {
        if self.terminal.is_some() {
            return Err(IrError::InvalidEventOrder);
        }
        if let EventIR::Started { .. } = event {
            if self.started {
                return Err(IrError::InvalidEventOrder);
            }
            self.started = true;
            return Ok(());
        }
        if !self.started {
            return Err(IrError::InvalidEventOrder);
        }
        match event {
            EventIR::Started { .. } => unreachable!(),
            EventIR::ItemStarted { id, index, kind } => {
                if self.items.len() >= self.limits.max_items {
                    return Err(IrError::SizeLimit);
                }
                if self.items.contains_key(&id) || self.indices.contains(&index) {
                    return Err(IrError::DuplicateId);
                }
                if let OutputKind::Tool { tool, call_id, .. } = &kind {
                    tool.validate()?;
                    if self.calls.contains(call_id) {
                        return Err(IrError::DuplicateId);
                    }
                    self.calls.insert(call_id.clone());
                }
                self.indices.insert(index);
                self.items.insert(
                    id,
                    ItemState {
                        kind,
                        open: true,
                        parts: BTreeMap::new(),
                        arguments: String::new(),
                    },
                );
            }
            EventIR::PartStarted { item, index, kind } => {
                if self.parts >= self.limits.max_parts {
                    return Err(IrError::SizeLimit);
                }
                let state = self.items.get_mut(&item).ok_or(IrError::UnknownItem)?;
                if !state.open
                    || !matches!(
                        (&state.kind, kind),
                        (OutputKind::Message, PartKind::Text)
                            | (OutputKind::Reasoning, PartKind::ReasoningText)
                    )
                {
                    return Err(IrError::InvalidEventOrder);
                }
                if state.parts.contains_key(&index) {
                    return Err(IrError::DuplicateId);
                }
                state.parts.insert(index, PartState { open: true });
                self.parts += 1;
            }
            EventIR::TextDelta { item, index, text } => {
                if text.len() > self.limits.max_delta_bytes {
                    return Err(IrError::SizeLimit);
                }
                let state = self.items.get(&item).ok_or(IrError::UnknownItem)?;
                if !state.open || !state.parts.get(&index).is_some_and(|p| p.open) {
                    return Err(IrError::InvalidEventOrder);
                }
                // Text is forwarded by the caller, not accumulated in the validator.
            }
            EventIR::PartFinished { item, index } => {
                let state = self.items.get_mut(&item).ok_or(IrError::UnknownItem)?;
                let part = state
                    .parts
                    .get_mut(&index)
                    .ok_or(IrError::InvalidEventOrder)?;
                if !state.open || !part.open {
                    return Err(IrError::InvalidEventOrder);
                }
                part.open = false;
            }
            EventIR::ArgumentsDelta { item, text } => {
                if text.len() > self.limits.max_delta_bytes
                    || text.len()
                        > self
                            .limits
                            .max_argument_bytes
                            .saturating_sub(self.argument_bytes)
                {
                    return Err(IrError::SizeLimit);
                }
                let state = self.items.get_mut(&item).ok_or(IrError::UnknownItem)?;
                if !state.open || !matches!(state.kind, OutputKind::Tool { .. }) {
                    return Err(IrError::InvalidEventOrder);
                }
                state.arguments.push_str(&text);
                self.argument_bytes += text.len();
            }
            EventIR::ItemFinished { item } => {
                let state = self.items.get_mut(&item).ok_or(IrError::UnknownItem)?;
                if !state.open || state.parts.values().any(|p| p.open) {
                    return Err(IrError::InvalidEventOrder);
                }
                if matches!(
                    state.kind,
                    OutputKind::Tool {
                        kind: ToolKind::Function,
                        ..
                    }
                ) {
                    serde_json::from_str::<Value>(&state.arguments)
                        .map_err(|_| IrError::InvalidJsonArguments)?;
                }
                state.open = false;
            }
            EventIR::UsageUpdated(usage) => {
                if let Some(previous) = &self.usage {
                    for (old, new) in [
                        (previous.input_tokens, usage.input_tokens),
                        (previous.output_tokens, usage.output_tokens),
                    ] {
                        if matches!((old, new), (Some(a), Some(b)) if b < a) {
                            return Err(IrError::InvalidEventOrder);
                        }
                    }
                }
                let previous = self.usage.as_ref();
                self.usage = Some(Usage {
                    input_tokens: usage
                        .input_tokens
                        .or_else(|| previous.and_then(|p| p.input_tokens)),
                    output_tokens: usage
                        .output_tokens
                        .or_else(|| previous.and_then(|p| p.output_tokens)),
                });
            }
            EventIR::Finished {
                status: terminal,
                reason,
            } => {
                if reason.as_ref().is_some_and(|r| r.len() > 1024) {
                    return Err(IrError::SizeLimit);
                }
                if terminal == Terminal::Completed && self.items.values().any(|i| i.open) {
                    return Err(IrError::InvalidEventOrder);
                }
                self.terminal = Some(terminal);
                self.terminal_reason = reason;
            }
        }
        Ok(())
    }
}
