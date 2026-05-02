/// Stateful parser that splits a streamed content body on `<thought>`/
/// `<think>`-style tag boundaries. Used by OpenAI-compatible providers
/// (Gemini, MiniMax) that wrap chain-of-thought inside their normal
/// `delta.content` field rather than offering a dedicated reasoning channel.
///
/// Carries a small buffer between chunks so that tags split across chunk
/// boundaries (e.g. "<tho" + "ught>") are still detected.
#[derive(Debug, PartialEq)]
pub enum Segment {
    Thinking(String),
    Answer(String),
}

pub struct TagSplitter {
    open_tag: &'static str,
    close_tag: &'static str,
    in_thinking: bool,
    buffer: String,
}

impl TagSplitter {
    pub fn new(open_tag: &'static str, close_tag: &'static str) -> Self {
        Self {
            open_tag,
            close_tag,
            in_thinking: false,
            buffer: String::new(),
        }
    }

    pub fn in_thinking(&self) -> bool {
        self.in_thinking
    }

    pub fn push(&mut self, chunk: &str) -> Vec<Segment> {
        self.buffer.push_str(chunk);
        let mut out = Vec::new();
        loop {
            let target = if self.in_thinking {
                self.close_tag
            } else {
                self.open_tag
            };
            if let Some(idx) = self.buffer.find(target) {
                if idx > 0 {
                    let segment: String = self.buffer.drain(..idx).collect();
                    out.push(self.classify(segment));
                }
                self.buffer.drain(..target.len());
                self.in_thinking = !self.in_thinking;
                // After closing a thinking block, drop a single trailing newline
                // for parity with the previous extract_think_tags behavior.
                if !self.in_thinking && self.buffer.starts_with('\n') {
                    self.buffer.drain(..1);
                }
            } else {
                let hold = partial_suffix_len(&self.buffer, target);
                let emit_len = self.buffer.len() - hold;
                if emit_len > 0 {
                    let segment: String = self.buffer.drain(..emit_len).collect();
                    out.push(self.classify(segment));
                }
                break;
            }
        }
        out
    }

    pub fn flush(mut self) -> Option<Segment> {
        if self.buffer.is_empty() {
            None
        } else {
            let text = std::mem::take(&mut self.buffer);
            Some(self.classify(text))
        }
    }

    fn classify(&self, text: String) -> Segment {
        if self.in_thinking {
            Segment::Thinking(text)
        } else {
            Segment::Answer(text)
        }
    }
}

/// How much of `tag`'s prefix appears at the end of `buf` — bytes we must
/// hold back in case the next chunk completes the tag.
fn partial_suffix_len(buf: &str, tag: &str) -> usize {
    let max = std::cmp::min(tag.len() - 1, buf.len());
    for i in (1..=max).rev() {
        if buf.ends_with(&tag[..i]) {
            return i;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitter_full_thought_and_answer_in_separate_chunks() {
        let mut p = TagSplitter::new("<thought>", "</thought>");
        let segs = p.push("<thought>plan A</thought>answer");
        assert_eq!(
            segs,
            vec![
                Segment::Thinking("plan A".into()),
                Segment::Answer("answer".into())
            ]
        );
    }

    #[test]
    fn splitter_split_open_tag_across_chunks() {
        let mut p = TagSplitter::new("<thought>", "</thought>");
        assert_eq!(p.push("<tho"), vec![]);
        assert_eq!(p.push("ught>plan"), vec![Segment::Thinking("plan".into())]);
    }

    #[test]
    fn splitter_split_close_tag_across_chunks() {
        let mut p = TagSplitter::new("<thought>", "</thought>");
        let _ = p.push("<thought>plan");
        assert_eq!(p.push("</thou"), vec![]);
        assert_eq!(p.push("ght>answer"), vec![Segment::Answer("answer".into())]);
    }

    #[test]
    fn splitter_close_tag_at_start_of_answer_chunk() {
        // Mirrors the real Gemini boundary: thought chunk has only opening
        // tag, answer chunk starts with closing tag.
        let mut p = TagSplitter::new("<thought>", "</thought>");
        assert_eq!(
            p.push("<thought>thinking text"),
            vec![Segment::Thinking("thinking text".into())]
        );
        assert_eq!(
            p.push("</thought>**Answer**"),
            vec![Segment::Answer("**Answer**".into())]
        );
    }

    #[test]
    fn splitter_no_tags_passthrough() {
        let mut p = TagSplitter::new("<thought>", "</thought>");
        assert_eq!(
            p.push("plain answer text"),
            vec![Segment::Answer("plain answer text".into())]
        );
        assert!(p.flush().is_none());
    }

    #[test]
    fn splitter_strips_trailing_newline_after_close() {
        let mut p = TagSplitter::new("<think>", "</think>");
        let segs = p.push("<think>x</think>\nanswer");
        assert_eq!(
            segs,
            vec![
                Segment::Thinking("x".into()),
                Segment::Answer("answer".into())
            ]
        );
    }

    #[test]
    fn splitter_holds_partial_suffix_that_is_not_tag() {
        // A trailing '<' could start an open tag; must be held back.
        let mut p = TagSplitter::new("<thought>", "</thought>");
        let s1 = p.push("hello <");
        assert_eq!(s1, vec![Segment::Answer("hello ".into())]);
        let s2 = p.push("world");
        assert_eq!(s2, vec![Segment::Answer("<world".into())]);
    }

    #[test]
    fn splitter_unicode_safe_when_buffer_ends_non_ascii() {
        let mut p = TagSplitter::new("<thought>", "</thought>");
        let segs = p.push("café 🍰");
        assert_eq!(segs, vec![Segment::Answer("café 🍰".into())]);
    }
}
