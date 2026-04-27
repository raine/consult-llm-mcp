/// Minimal SSE (Server-Sent Events) parser sufficient for the streaming chat-
/// completion endpoints we consume. We do not implement reconnection or
/// Last-Event-ID — those features are unused by every backend in this crate.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SseEvent {
    pub data: String,
    pub event: Option<String>,
}

/// Cap on the unparsed buffer size between event boundaries. A misbehaving
/// or malicious server that streams bytes without ever closing a frame would
/// otherwise grow the buffer until the idle timeout fires (potentially
/// exhausting memory if the bytes keep flowing). 16 MiB is well above any
/// realistic single SSE frame for chat completion responses.
const MAX_BUFFER_BYTES: usize = 16 * 1024 * 1024;

#[derive(Default)]
pub struct SseParser {
    buf: Vec<u8>,
    cur_data: String,
    cur_event: Option<String>,
    have_field: bool,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed bytes from the wire. Returns any complete events that became
    /// available. Holds back partial frames across calls so split delimiters
    /// (`\r\n\r\n` cut between two reads) are still detected. Errors if the
    /// unparsed buffer would exceed `MAX_BUFFER_BYTES` — guards against a
    /// peer that streams without ever closing a frame.
    pub fn feed(&mut self, chunk: &[u8]) -> anyhow::Result<Vec<SseEvent>> {
        self.buf.extend_from_slice(chunk);
        if self.buf.len() > MAX_BUFFER_BYTES {
            anyhow::bail!(
                "SSE frame exceeded {} bytes without a delimiter",
                MAX_BUFFER_BYTES
            );
        }
        let mut out = Vec::new();
        loop {
            let Some((content_end, delim_len)) = find_double_newline(&self.buf) else {
                break;
            };
            let frame: Vec<u8> = self.buf.drain(..content_end).collect();
            self.buf.drain(..delim_len);
            // Use lossy decoding so a single invalid byte doesn't drop the whole
            // event; degrades to U+FFFD for the offending bytes.
            let text = String::from_utf8_lossy(&frame);
            for raw in text.split('\n') {
                let line = raw.strip_suffix('\r').unwrap_or(raw);
                if line.is_empty() {
                    continue;
                }
                if line.starts_with(':') {
                    continue;
                }
                let (field, value) = match line.split_once(':') {
                    Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
                    None => (line, ""),
                };
                self.have_field = true;
                match field {
                    "data" => {
                        if !self.cur_data.is_empty() {
                            self.cur_data.push('\n');
                        }
                        self.cur_data.push_str(value);
                    }
                    "event" => self.cur_event = Some(value.to_string()),
                    _ => {}
                }
            }
            // Per SSE spec, events with an empty data buffer are NOT
            // dispatched — heartbeat-style `event: ping\n\n` frames are
            // structural noise we want to ignore.
            if self.have_field && !self.cur_data.is_empty() {
                out.push(SseEvent {
                    data: std::mem::take(&mut self.cur_data),
                    event: self.cur_event.take(),
                });
            }
            self.have_field = false;
            self.cur_data.clear();
            self.cur_event = None;
        }
        Ok(out)
    }

    /// Emit any remaining buffered event when the stream ends without a
    /// trailing blank line. Reserved for callers that drive `feed` to EOF.
    #[allow(dead_code)]
    pub fn flush(mut self) -> Option<SseEvent> {
        // Drain any unparsed residual lines.
        if !self.buf.is_empty() {
            let text = String::from_utf8_lossy(&self.buf);
            for raw in text.split('\n') {
                let line = raw.strip_suffix('\r').unwrap_or(raw);
                if line.is_empty() || line.starts_with(':') {
                    continue;
                }
                let (field, value) = match line.split_once(':') {
                    Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
                    None => (line, ""),
                };
                self.have_field = true;
                match field {
                    "data" => {
                        if !self.cur_data.is_empty() {
                            self.cur_data.push('\n');
                        }
                        self.cur_data.push_str(value);
                    }
                    "event" => self.cur_event = Some(value.to_string()),
                    _ => {}
                }
            }
            self.buf.clear();
        }
        if !self.have_field || self.cur_data.is_empty() {
            return None;
        }
        Some(SseEvent {
            data: std::mem::take(&mut self.cur_data),
            event: self.cur_event.take(),
        })
    }
}

/// Returns (content_end, delimiter_len) where the event terminates.
/// Recognizes `\n\n` and `\r\n\r\n` (mixed `\r\n\n` and `\n\r\n` are not part of
/// the SSE spec but we match `\n\n` first which covers both real-world cases).
fn find_double_newline(buf: &[u8]) -> Option<(usize, usize)> {
    // Search jointly for the earliest delimiter to avoid bias.
    let mut best: Option<(usize, usize)> = None;
    if let Some(i) = find_subslice(buf, b"\n\n") {
        best = Some((i, 2));
    }
    if let Some(i) = find_subslice(buf, b"\r\n\r\n") {
        match best {
            Some((j, _)) if j < i => {}
            _ => best = Some((i, 4)),
        }
    }
    best
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(p: &mut SseParser, chunk: &[u8]) -> Vec<SseEvent> {
        p.feed(chunk).expect("feed ok")
    }

    #[test]
    fn basic_event() {
        let mut p = SseParser::new();
        let evts = feed(&mut p, b"data: hello\n\n");
        assert_eq!(evts.len(), 1);
        assert_eq!(evts[0].data, "hello");
    }

    #[test]
    fn split_across_chunks() {
        let mut p = SseParser::new();
        assert!(feed(&mut p, b"data: he").is_empty());
        assert!(feed(&mut p, b"llo\n").is_empty());
        let evts = feed(&mut p, b"\n");
        assert_eq!(evts.len(), 1);
        assert_eq!(evts[0].data, "hello");
    }

    #[test]
    fn split_delimiter_across_chunks() {
        // The blank-line delimiter itself spans two reads.
        let mut p = SseParser::new();
        assert!(feed(&mut p, b"data: a\n").is_empty());
        let evts = feed(&mut p, b"\ndata: b\n\n");
        assert_eq!(evts.len(), 2);
        assert_eq!(evts[0].data, "a");
        assert_eq!(evts[1].data, "b");
    }

    #[test]
    fn crlf_delimiter() {
        let mut p = SseParser::new();
        let evts = feed(&mut p, b"data: hi\r\n\r\n");
        assert_eq!(evts.len(), 1);
        assert_eq!(evts[0].data, "hi");
    }

    #[test]
    fn crlf_delimiter_split() {
        let mut p = SseParser::new();
        assert!(feed(&mut p, b"data: hi\r\n\r").is_empty());
        let evts = feed(&mut p, b"\n");
        assert_eq!(evts.len(), 1);
        assert_eq!(evts[0].data, "hi");
    }

    #[test]
    fn multiline_data() {
        let mut p = SseParser::new();
        let evts = feed(&mut p, b"data: line1\ndata: line2\n\n");
        assert_eq!(evts.len(), 1);
        assert_eq!(evts[0].data, "line1\nline2");
    }

    #[test]
    fn comments_and_pings() {
        let mut p = SseParser::new();
        let evts = feed(&mut p, b": ping\n\ndata: real\n\n");
        // The ":ping" frame has no fields → not emitted.
        assert_eq!(evts.len(), 1);
        assert_eq!(evts[0].data, "real");
    }

    #[test]
    fn event_field_without_data_is_not_dispatched() {
        // SSE spec: empty data buffer → event is not dispatched.
        let mut p = SseParser::new();
        assert!(feed(&mut p, b"event: ping\n\n").is_empty());
    }

    #[test]
    fn event_field_captured() {
        let mut p = SseParser::new();
        let evts = feed(&mut p, b"event: ping\ndata: {}\n\n");
        assert_eq!(evts.len(), 1);
        assert_eq!(evts[0].event.as_deref(), Some("ping"));
        assert_eq!(evts[0].data, "{}");
    }

    #[test]
    fn flush_emits_trailing_event_without_blank_line() {
        let mut p = SseParser::new();
        assert!(feed(&mut p, b"data: tail").is_empty());
        let last = p.flush().expect("trailing event");
        assert_eq!(last.data, "tail");
    }

    #[test]
    fn flush_empty_returns_none() {
        let p = SseParser::new();
        assert!(p.flush().is_none());
    }

    #[test]
    fn data_field_with_no_space() {
        let mut p = SseParser::new();
        let evts = feed(&mut p, b"data:hello\n\n");
        assert_eq!(evts.len(), 1);
        assert_eq!(evts[0].data, "hello");
    }

    #[test]
    fn data_done_sentinel() {
        let mut p = SseParser::new();
        let evts = feed(&mut p, b"data: [DONE]\n\n");
        assert_eq!(evts.len(), 1);
        assert_eq!(evts[0].data, "[DONE]");
    }

    #[test]
    fn invalid_utf8_replaced_lossily() {
        let mut p = SseParser::new();
        let mut bytes = b"data: pre".to_vec();
        bytes.extend_from_slice(&[0xff, 0xff]);
        bytes.extend_from_slice(b"post\n\n");
        let evts = feed(&mut p, &bytes);
        assert_eq!(evts.len(), 1);
        assert!(evts[0].data.contains("pre"));
        assert!(evts[0].data.contains("post"));
    }

    #[test]
    fn errors_when_buffer_exceeds_cap() {
        let mut p = SseParser::new();
        // Stream bytes without ever closing a frame.
        let chunk = vec![b'x'; 1024 * 1024];
        for _ in 0..16 {
            let _ = p.feed(&chunk);
        }
        assert!(p.feed(&chunk).is_err());
    }
}
