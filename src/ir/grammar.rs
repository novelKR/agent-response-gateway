//! Exact registry selection and syntax-only validation. Never accesses the filesystem.
use serde_json::Value;

use super::IrError;

pub const PATCH_GRAMMAR_SHA256: [u8; 32] = [
    0xd6, 0x36, 0x7f, 0x48, 0x26, 0xed, 0x60, 0x8c, 0x42, 0x4b, 0x0a, 0x30, 0x8f, 0x3d, 0x61, 0x63,
    0x52, 0x7d, 0xf6, 0x3c, 0x22, 0x51, 0x3d, 0x08, 0x9b, 0x91, 0x86, 0x35, 0x52, 0xf8, 0xbf, 0xeb,
];

pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    ring::digest::digest(&ring::digest::SHA256, bytes)
        .as_ref()
        .try_into()
        .expect("SHA-256 length")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grammar {
    Text,
    CodexPatchV1,
}

impl Grammar {
    pub fn from_format(format: Option<&Value>) -> Result<Self, IrError> {
        let Some(format) = format else {
            return Ok(Self::Text);
        };
        let fields = format.as_object().ok_or(IrError::UnsupportedFeature)?;
        match format.get("type").and_then(Value::as_str) {
            Some("text") if fields.len() == 1 => Ok(Self::Text),
            Some("grammar")
                if fields.len() == 3
                    && format.get("syntax").and_then(Value::as_str) == Some("lark") =>
            {
                let definition = format
                    .get("definition")
                    .and_then(Value::as_str)
                    .ok_or(IrError::UnsupportedFeature)?;
                if sha256(definition.as_bytes()) != PATCH_GRAMMAR_SHA256 {
                    return Err(IrError::UnsupportedFeature);
                }
                Ok(Self::CodexPatchV1)
            }
            _ => Err(IrError::UnsupportedFeature),
        }
    }

    pub fn version(self) -> &'static str {
        match self {
            Self::Text => "text/1",
            Self::CodexPatchV1 => "codex-patch/1",
        }
    }

    pub fn validate(self, text: &str) -> Result<(), IrError> {
        if text.len() > 8 * 1024 * 1024 {
            return Err(IrError::SizeLimit);
        }
        if self == Self::Text {
            return Ok(());
        }
        let fail = || IrError::InvalidField("custom_tool_grammar");
        let text = text.strip_suffix('\n').unwrap_or(text);
        let body = text
            .strip_prefix("*** Begin Patch\n")
            .and_then(|v| v.strip_suffix("*** End Patch"))
            .ok_or_else(fail)?;
        if !body.ends_with('\n') {
            return Err(fail());
        }
        let mut lines = body.split_terminator('\n').peekable();
        let mut hunks = 0;
        let path =
            |line: &str, prefix: &str| line.strip_prefix(prefix).is_some_and(|v| !v.is_empty());
        while let Some(line) = lines.next() {
            if path(line, "*** Add File: ") {
                let mut count = 0;
                while lines.peek().is_some_and(|line| line.starts_with('+')) {
                    lines.next();
                    count += 1;
                }
                if count == 0 {
                    return Err(fail());
                }
            } else if path(line, "*** Delete File: ") {
                // Syntax only: ownership, existence and deletion permission belong to the host.
            } else if path(line, "*** Update File: ") {
                if lines.peek().is_some_and(|line| path(line, "*** Move to: ")) {
                    lines.next();
                }
                let mut changed = false;
                while let Some(line) = lines.peek() {
                    if *line == "@@"
                        || line.strip_prefix("@@ ").is_some_and(|s| !s.is_empty())
                        || line.starts_with(['+', '-', ' '])
                    {
                        lines.next();
                        changed = true;
                    } else {
                        break;
                    }
                }
                if lines.peek() == Some(&"*** End of File") {
                    if !changed {
                        return Err(fail());
                    }
                    lines.next();
                }
            } else {
                return Err(fail());
            }
            hunks += 1;
        }
        if hunks == 0 {
            return Err(fail());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn digest_matches_standard_vectors_and_unknown_formats_reject() {
        let hex = |value: [u8; 32]| value.iter().map(|b| format!("{b:02x}")).collect::<String>();
        assert_eq!(
            hex(sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(
            Grammar::from_format(Some(
                &serde_json::json!({"type":"grammar","syntax":"lark","definition":"start: /.+/"})
            ))
            .is_err()
        );
        assert_eq!(Grammar::from_format(None).unwrap(), Grammar::Text);
    }
    #[test]
    fn patch_syntax_handles_hunks_and_rejects_malformed_boundaries() {
        for patch in [
            "*** Begin Patch\n*** Add File: synthetic.txt\n+한글\n+\n*** End Patch",
            "*** Begin Patch\n*** Delete File: synthetic.txt\n*** End Patch\n",
            "*** Begin Patch\n*** Update File: a\n*** Move to: b\n@@ context\n-old\n+new\n*** End of File\n*** End Patch\n",
            "*** Begin Patch\n*** Update File: a\n*** End Patch",
        ] {
            Grammar::CodexPatchV1.validate(patch).unwrap();
        }
        for patch in [
            "*** Begin Patch\n*** End Patch",
            "*** Begin Patch\n*** Add File: a\n*** End Patch",
            "*** Begin Patch\n*** Delete File: \n*** End Patch",
            "*** Begin Patch\n*** Update File: a\n*** End of File\n*** End Patch",
            "*** Begin Patch\n*** Add File: a\n+one\n*** End Patch\ntrailing",
            "*** Begin Patch\r\n*** Delete File: a\r\n*** End Patch",
            "*** Begin Patch\n*** Update File: a\n@@ \n+x\n*** End Patch",
        ] {
            assert!(Grammar::CodexPatchV1.validate(patch).is_err());
        }
    }
}
