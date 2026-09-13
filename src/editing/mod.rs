//! Opt-in pure editing representations. File access and execution belong to the host.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::ir::{IrError, grammar::Grammar};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub version: u32,
    pub client_contract: ClientContract,
    pub representation: Representation,
    pub patch_dialect: PatchDialect,
    pub normalization: Normalization,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientContract {
    #[serde(rename = "codex-direct-custom/v1")]
    Direct,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Representation {
    #[serde(rename = "context-lines/v1")]
    ContextLines,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PatchDialect {
    #[serde(rename = "codex-patch/1")]
    CodexPatch,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Normalization {
    #[serde(rename = "none")]
    None,
}

impl Policy {
    pub fn validate(&self) -> Result<(), IrError> {
        if self.version != 1 {
            return Err(IrError::UnsupportedVersion);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextEdit {
    pub path: String,
    pub before_context: Vec<String>,
    pub old_lines: Vec<String>,
    pub new_lines: Vec<String>,
    pub after_context: Vec<String>,
}

fn invalid() -> IrError {
    IrError::InvalidField("structured_edit")
}

impl ContextEdit {
    pub fn from_json(raw: &str) -> Result<Self, IrError> {
        if raw.len() > 8 * 1024 * 1024 {
            return Err(IrError::SizeLimit);
        }
        // Deserialize directly: unlike Value-first decoding this rejects duplicate fields.
        serde_json::from_str(raw).map_err(|_| invalid())
    }

    pub fn compile(&self) -> Result<String, IrError> {
        if self.path.is_empty()
            || self.path.trim() != self.path
            || self.path.chars().any(char::is_control)
            || self.old_lines == self.new_lines
            || self.old_lines.is_empty()
        {
            return Err(invalid());
        }
        let lines = [
            &self.before_context,
            &self.old_lines,
            &self.new_lines,
            &self.after_context,
        ];
        if lines.iter().map(|v| v.len()).sum::<usize>() > 16384
            || lines
                .iter()
                .flat_map(|v| v.iter())
                .any(|s| s.contains(['\n', '\r', '\0']))
        {
            return Err(invalid());
        }
        let mut patch = format!("*** Begin Patch\n*** Update File: {}\n@@\n", self.path);
        for (prefix, lines) in [
            (' ', &self.before_context),
            ('-', &self.old_lines),
            ('+', &self.new_lines),
            (' ', &self.after_context),
        ] {
            for line in lines {
                patch.push(prefix);
                patch.push_str(line);
                patch.push('\n');
                if patch.len() > 8 * 1024 * 1024 {
                    return Err(IrError::SizeLimit);
                }
            }
        }
        patch.push_str("*** End Patch");
        Grammar::CodexPatchV1.validate(&patch)?;
        Ok(patch)
    }

    /// Decode only our canonical representation, never infer edits from arbitrary patches.
    pub fn from_patch(patch: &str) -> Result<Self, IrError> {
        let body = patch
            .strip_prefix("*** Begin Patch\n*** Update File: ")
            .ok_or_else(invalid)?;
        let (path, body) = body.split_once("\n@@\n").ok_or_else(invalid)?;
        let body = body.strip_suffix("*** End Patch").ok_or_else(invalid)?;
        let mut value = Self {
            path: path.into(),
            before_context: vec![],
            old_lines: vec![],
            new_lines: vec![],
            after_context: vec![],
        };
        let mut phase = 0;
        for line in body.split_terminator('\n') {
            let (prefix, text) = line.split_at_checked(1).ok_or_else(invalid)?;
            match prefix {
                " " if phase == 0 => value.before_context.push(text.into()),
                "-" if phase <= 1 => {
                    phase = 1;
                    value.old_lines.push(text.into());
                }
                "+" if (1..=2).contains(&phase) => {
                    phase = 2;
                    value.new_lines.push(text.into());
                }
                " " if phase >= 1 => {
                    phase = 3;
                    value.after_context.push(text.into());
                }
                _ => return Err(invalid()),
            }
        }
        if value.compile()? != patch {
            return Err(invalid());
        }
        Ok(value)
    }

    pub fn schema() -> Value {
        let lines = json!({"type":"array","items":{"type":"string"}});
        json!({"type":"object","properties":{"path":{"type":"string"},"before_context":lines,"old_lines":lines,"new_lines":lines,"after_context":lines},"required":["path","before_context","old_lines","new_lines","after_context"],"additionalProperties":false})
    }
}
