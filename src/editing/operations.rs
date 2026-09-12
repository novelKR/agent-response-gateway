//! Independent file operations. This compiler never reads a filesystem.
use super::{ContextEdit, invalid};
use crate::ir::{IrError, grammar::Grammar};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationBundle {
    pub operations: Vec<Operation>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Create {
        path: String,
        lines: Vec<String>,
    },
    Delete {
        path: String,
    },
    Move {
        source: String,
        destination: String,
        context: Vec<String>,
    },
    Update {
        edit: ContextEdit,
    },
}

// Conservative portable lexical comparison, not a claim about inode identity.
fn path_key(path: &str) -> Result<String, IrError> {
    if path.is_empty() || path.trim() != path || path.chars().any(char::is_control) {
        return Err(invalid());
    }
    let normalized = path.replace('\\', "/");
    let mut parts = Vec::new();
    for part in normalized.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop().ok_or_else(invalid)?;
            }
            _ => {
                if part.ends_with([' ', '.']) {
                    return Err(invalid());
                }
                parts.push(part.to_lowercase());
            }
        }
    }
    if parts.is_empty() {
        return Err(invalid());
    }
    Ok(format!(
        "{}{}",
        if normalized.starts_with('/') { "/" } else { "" },
        parts.join("/")
    ))
}

fn lines_valid(lines: &[String]) -> Result<(), IrError> {
    if lines.is_empty()
        || lines.len() > 16384
        || lines.iter().any(|s| s.contains(['\n', '\r', '\0']))
    {
        return Err(invalid());
    }
    Ok(())
}

impl OperationBundle {
    pub fn from_json(raw: &str) -> Result<Self, IrError> {
        if raw.len() > 8 * 1024 * 1024 {
            return Err(IrError::SizeLimit);
        }
        serde_json::from_str(raw).map_err(|_| invalid())
    }

    pub fn compile(&self) -> Result<String, IrError> {
        if self.operations.is_empty() || self.operations.len() > 64 {
            return Err(invalid());
        }
        let mut paths: Vec<String> = Vec::new();
        let mut patch = String::from("*** Begin Patch\n");
        let mut line_count = 0;
        for operation in &self.operations {
            let touched = match operation {
                Operation::Create { path, .. } | Operation::Delete { path } => vec![path],
                Operation::Move {
                    source,
                    destination,
                    ..
                } => vec![source, destination],
                Operation::Update { edit } => vec![&edit.path],
            };
            for path in touched {
                let key = path_key(path)?;
                if paths.iter().any(|old| {
                    old == &key
                        || old.starts_with(&format!("{key}/"))
                        || key.starts_with(&format!("{old}/"))
                }) {
                    return Err(invalid());
                }
                paths.push(key);
            }
            match operation {
                Operation::Create { path, lines } => {
                    lines_valid(lines)?;
                    patch.push_str(&format!("*** Add File: {path}\n"));
                    for line in lines {
                        patch.push_str(&format!("+{line}\n"));
                    }
                    line_count += lines.len();
                }
                Operation::Delete { path } => patch.push_str(&format!("*** Delete File: {path}\n")),
                Operation::Move {
                    source,
                    destination,
                    context,
                } => {
                    lines_valid(context)?;
                    patch.push_str(&format!(
                        "*** Update File: {source}\n*** Move to: {destination}\n@@\n"
                    ));
                    for line in context {
                        patch.push_str(&format!(" {line}\n"));
                    }
                    line_count += context.len();
                }
                Operation::Update { edit } => {
                    let compiled = edit.compile()?;
                    patch.push_str(
                        compiled
                            .strip_prefix("*** Begin Patch\n")
                            .and_then(|s| s.strip_suffix("*** End Patch"))
                            .ok_or_else(invalid)?,
                    );
                    line_count += edit.before_context.len()
                        + edit.old_lines.len()
                        + edit.new_lines.len()
                        + edit.after_context.len();
                }
            }
            if patch.len() > 8 * 1024 * 1024 || line_count > 16384 {
                return Err(IrError::SizeLimit);
            }
        }
        patch.push_str("*** End Patch");
        if patch.len() > 8 * 1024 * 1024 {
            return Err(IrError::SizeLimit);
        }
        Grammar::CodexPatchV1.validate(&patch)?;
        Ok(patch)
    }

    /// Invert only canonical output; ordinary legacy patches stay on the raw tool.
    pub fn from_patch(patch: &str) -> Result<Self, IrError> {
        if patch.len() > 8 * 1024 * 1024 {
            return Err(IrError::SizeLimit);
        }
        let body = patch
            .strip_prefix("*** Begin Patch\n")
            .and_then(|s| s.strip_suffix("*** End Patch"))
            .ok_or_else(invalid)?;
        let lines: Vec<&str> = body.split_terminator('\n').collect();
        let mut cursor = 0;
        let mut operations = Vec::new();
        while cursor < lines.len() {
            let start = cursor;
            cursor += 1;
            while cursor < lines.len()
                && !["*** Add File: ", "*** Delete File: ", "*** Update File: "]
                    .iter()
                    .any(|p| lines[cursor].starts_with(p))
            {
                cursor += 1;
            }
            let chunk = &lines[start..cursor];
            let operation = if let Some(path) = chunk[0].strip_prefix("*** Add File: ") {
                Operation::Create {
                    path: path.into(),
                    lines: chunk[1..]
                        .iter()
                        .map(|line| {
                            line.strip_prefix('+')
                                .map(str::to_owned)
                                .ok_or_else(invalid)
                        })
                        .collect::<Result<_, _>>()?,
                }
            } else if let Some(path) = chunk[0].strip_prefix("*** Delete File: ") {
                if chunk.len() != 1 {
                    return Err(invalid());
                }
                Operation::Delete { path: path.into() }
            } else if let Some(source) = chunk[0].strip_prefix("*** Update File: ") {
                if let Some(destination) =
                    chunk.get(1).and_then(|s| s.strip_prefix("*** Move to: "))
                {
                    if chunk.get(2) != Some(&"@@") {
                        return Err(invalid());
                    }
                    Operation::Move {
                        source: source.into(),
                        destination: destination.into(),
                        context: chunk[3..]
                            .iter()
                            .map(|line| {
                                line.strip_prefix(' ')
                                    .map(str::to_owned)
                                    .ok_or_else(invalid)
                            })
                            .collect::<Result<_, _>>()?,
                    }
                } else {
                    Operation::Update {
                        edit: ContextEdit::from_patch(&format!(
                            "*** Begin Patch\n{}\n*** End Patch",
                            chunk.join("\n")
                        ))?,
                    }
                }
            } else {
                return Err(invalid());
            };
            operations.push(operation);
            if operations.len() > 64 {
                return Err(IrError::SizeLimit);
            }
        }
        let value = Self { operations };
        if value.compile()? != patch {
            return Err(invalid());
        }
        Ok(value)
    }

    pub fn schema() -> Value {
        let string = json!({"type":"string"});
        let lines = json!({"type":"array","items":string});
        let variants = [
            ("create",json!({"path":string,"lines":lines})),
            ("delete",json!({"path":string})),
            ("move",json!({"source":string,"destination":string,"context":lines})),
            ("update",json!({"edit":ContextEdit::schema()})),
        ].into_iter().map(|(name,mut properties)| {
            let map = properties.as_object_mut().expect("schema object");
            map.insert("operation".into(),json!({"type":"string","enum":[name]}));
            let required:Vec<_> = map.keys().cloned().collect();
            json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
        }).collect::<Vec<_>>();
        json!({"type":"object","properties":{"operations":{"type":"array","items":{"anyOf":variants}}},"required":["operations"],"additionalProperties":false})
    }
}
