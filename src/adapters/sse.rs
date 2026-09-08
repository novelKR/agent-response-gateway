//! Bounded SSE framing across arbitrary byte/UTF-8/line boundaries.
use crate::ir::IrError;

pub struct SseEvent {
    pub event: String,
    pub data: String,
}

pub struct SseDecoder {
    line: Vec<u8>,
    data: String,
    event: String,
    has_data: bool,
    first_line: bool,
    after_cr: bool,
    limit: usize,
}

impl SseDecoder {
    pub fn new(limit: usize) -> Result<Self, IrError> {
        if limit == 0 {
            return Err(IrError::SizeLimit);
        }
        Ok(Self {
            line: Vec::new(),
            data: String::new(),
            event: String::new(),
            has_data: false,
            first_line: true,
            after_cr: false,
            limit,
        })
    }

    /// Consume at most one complete event, retaining the remainder in `input`.
    /// A caller can yield downstream before parsing more bytes from the same chunk.
    pub fn next_event(&mut self, input: &mut &[u8]) -> Result<Option<SseEvent>, IrError> {
        while let Some((&byte, rest)) = input.split_first() {
            *input = rest;
            if self.after_cr {
                self.after_cr = false;
                if byte == b'\n' {
                    continue;
                }
            }
            if matches!(byte, b'\n' | b'\r') {
                self.after_cr = byte == b'\r';
                if let Some(event) = self.end_line()? {
                    return Ok(Some(event));
                }
            } else {
                if self
                    .line
                    .len()
                    .saturating_add(self.data.len())
                    .saturating_add(self.event.len())
                    >= self.limit
                {
                    return Err(IrError::SizeLimit);
                }
                self.line.push(byte);
            }
        }
        Ok(None)
    }

    fn end_line(&mut self) -> Result<Option<SseEvent>, IrError> {
        let line = std::mem::take(&mut self.line);
        let line = if self.first_line && line.starts_with(&[0xef, 0xbb, 0xbf]) {
            &line[3..]
        } else {
            &line
        };
        self.first_line = false;
        let line = std::str::from_utf8(line).map_err(|_| IrError::InvalidField("sse_utf8"))?;
        if line.is_empty() {
            if !self.has_data {
                self.event.clear();
                return Ok(None);
            }
            self.has_data = false;
            self.data.pop(); // SSE joins each data line with one LF, excluding the final LF.
            let event = if self.event.is_empty() {
                "message".into()
            } else {
                std::mem::take(&mut self.event)
            };
            return Ok(Some(SseEvent {
                event,
                data: std::mem::take(&mut self.data),
            }));
        }
        if line.starts_with(':') {
            return Ok(None);
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "event" => {
                self.event = value.into();
            }
            "data" => {
                if self
                    .data
                    .len()
                    .saturating_add(value.len())
                    .saturating_add(1)
                    .saturating_add(self.event.len())
                    > self.limit
                {
                    return Err(IrError::SizeLimit);
                }
                self.data.push_str(value);
                self.data.push('\n');
                self.has_data = true;
            }
            // Reconnect IDs/retry intervals are never used to retry an upstream request.
            _ => {}
        }
        Ok(None)
    }

    pub fn finish(&self) -> Result<(), IrError> {
        if self.has_data || !self.line.is_empty() || !self.event.is_empty() {
            return Err(IrError::InvalidEventOrder);
        }
        Ok(())
    }
}
