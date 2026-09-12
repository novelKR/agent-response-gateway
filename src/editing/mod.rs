//! Opt-in pure editing representations. File access and execution belong to the host.
mod operations;
pub use operations::{Operation, OperationBundle};

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_descriptor_sha256: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientContract {
    #[serde(rename = "codex-direct-custom/v1")]
    Direct,
    #[serde(rename = "codex-code-mode/v1")]
    CodeMode,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Representation {
    #[serde(rename = "operations/v1")]
    Operations,
    #[serde(rename = "patch-text/v1")]
    PatchText,
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
    #[serde(rename = "patch-envelope/v1")]
    PatchEnvelope,
}

impl Policy {
    pub fn validate(&self) -> Result<(), IrError> {
        let descriptor_valid = match self.client_contract {
            ClientContract::Direct => self.client_descriptor_sha256.is_none(),
            ClientContract::CodeMode => self.client_descriptor_sha256.as_ref().is_some_and(|s| {
                s.len() == 64
                    && s.bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            }),
        };
        if self.version != 1
            || !descriptor_valid
            || (self.client_contract == ClientContract::CodeMode
                && self.normalization != Normalization::None)
        {
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

/// Pinned runtime contract hashes, not a parser for arbitrary JavaScript.
pub const EXEC_GRAMMAR_SHA256: &str =
    "8eead048e20069c17fcd0b6fe5ee9ed29364868723f8e08889de146a2b4b7a8e";
const WRAPPER_PREFIX: &str = "const result = await tools.apply_patch(";
const WRAPPER_SUFFIX: &str = ");\ntext(result);";

pub fn helper_program(patch: &str) -> Result<String, IrError> {
    Grammar::CodexPatchV1.validate(patch)?;
    let program = format!(
        "{WRAPPER_PREFIX}{}{WRAPPER_SUFFIX}",
        serde_json::to_string(patch).map_err(|_| invalid())?
    );
    if program.len() > 8 * 1024 * 1024 {
        return Err(IrError::SizeLimit);
    }
    Ok(program)
}
pub fn helper_patch(program: &str) -> Result<String, IrError> {
    if program.len() > 8 * 1024 * 1024 {
        return Err(IrError::SizeLimit);
    }
    let encoded = program
        .strip_prefix(WRAPPER_PREFIX)
        .and_then(|s| s.strip_suffix(WRAPPER_SUFFIX))
        .ok_or_else(invalid)?;
    let patch: String = serde_json::from_str(encoded).map_err(|_| invalid())?;
    if helper_program(&patch)? != program {
        return Err(invalid());
    }
    Ok(patch)
}

/// Preserve the complete Code Mode program result as ordered text parts.
/// This does not interpret timing, success, helper results or arbitrary JSON in text.
pub fn code_mode_result(parts: &Value) -> Result<String, IrError> {
    let values = parts.as_array().ok_or_else(invalid)?;
    if values.len() > 4096
        || values.iter().any(|v| {
            v.as_object()
                .is_none_or(|m| m.len() != 2 || v["type"] != "input_text" || !v["text"].is_string())
        })
    {
        return Err(invalid());
    }
    let result = json!({"schema":"codex-exec-text-parts/v1","parts":parts}).to_string();
    if result.len() > 8 * 1024 * 1024 {
        return Err(IrError::SizeLimit);
    }
    Ok(result)
}
pub fn code_mode_result_parts(text: &str) -> Result<Value, IrError> {
    if text.len() > 8 * 1024 * 1024 {
        return Err(IrError::SizeLimit);
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Envelope {
        schema: String,
        parts: Value,
    }
    let value: Envelope = serde_json::from_str(text).map_err(|_| invalid())?;
    if value.schema != "codex-exec-text-parts/v1" || code_mode_result(&value.parts)? != text {
        return Err(invalid());
    }
    Ok(value.parts)
}
pub(crate) fn validate_code_mode_results(
    request: &crate::ir::request::RequestIR,
) -> Result<(), IrError> {
    use crate::ir::request::{Input, Item, ToolInput};
    let mut calls = std::collections::BTreeSet::new();
    if let Some(Input::Items(items)) = &request.input {
        for item in items {
            match item {
                Item::ToolCall(c)
                    if c.tool.name == "exec"
                        && c.tool.namespace.is_none()
                        && matches!(c.input, ToolInput::Freeform(_)) =>
                {
                    calls.insert(c.call_id.clone());
                }
                Item::ToolResult(r) if calls.contains(&r.call_id) => {
                    code_mode_result(&r.output)?;
                }
                Item::ToolResult(r) if !r.output.is_string() => {
                    return Err(IrError::UnsupportedFeature);
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// Rule metadata is separate from text and is not proof of execution success.
pub struct NormalizedPatch<'a> {
    pub text: std::borrow::Cow<'a, str>,
    pub applied_rule: Option<&'static str>,
}
#[derive(Debug, PartialEq, Eq)]
pub struct NormalizationEvidence {
    pub rule: Option<&'static str>,
    pub distinct_calls: usize,
}

pub fn normalize_patch_envelope(text: &str) -> Result<NormalizedPatch<'_>, IrError> {
    use std::borrow::Cow;
    if text.len() > 8 * 1024 * 1024 {
        return Err(IrError::SizeLimit);
    }
    let unchanged = || NormalizedPatch {
        text: Cow::Borrowed(text),
        applied_rule: None,
    };
    let terminal_lf = text.ends_with('\n');
    let body = text.strip_suffix('\n').unwrap_or(text);
    let Some((first, rest)) = body.split_once('\n') else {
        return Ok(unchanged());
    };
    let Some((middle, last)) = rest.rsplit_once('\n') else {
        return Ok(unchanged());
    };
    if !matches!(first, "*** Begin Patch" | "*** Begin Patch ***")
        || !matches!(last, "*** End Patch" | "*** End Patch ***")
        || (first == "*** Begin Patch" && last == "*** End Patch")
    {
        return Ok(unchanged());
    }
    let corrected = format!(
        "*** Begin Patch\n{middle}\n*** End Patch{}",
        if terminal_lf { "\n" } else { "" }
    );
    Grammar::CodexPatchV1.validate(&corrected)?;
    Ok(NormalizedPatch {
        text: Cow::Owned(corrected),
        applied_rule: Some("patch-envelope/v1"),
    })
}

impl Representation {
    pub(crate) fn compile(self, raw: &str) -> Result<String, IrError> {
        match self {
            Self::ContextLines => ContextEdit::from_json(raw)?.compile(),
            Self::Operations => OperationBundle::from_json(raw)?.compile(),
            Self::PatchText => Err(invalid()),
        }
    }
    pub(crate) fn decode(self, patch: &str) -> Result<String, IrError> {
        match self {
            Self::ContextLines => serde_json::to_string(&ContextEdit::from_patch(patch)?),
            Self::Operations => serde_json::to_string(&OperationBundle::from_patch(patch)?),
            Self::PatchText => return Err(invalid()),
        }
        .map_err(|_| invalid())
    }
    pub(crate) fn schema(self) -> Value {
        match self {
            Self::Operations => OperationBundle::schema(),
            _ => ContextEdit::schema(),
        }
    }
}
