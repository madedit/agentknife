//! Splits `<think>…</think>` reasoning out of streamed content. Some local models
//! (deepseek-r1, qwen3 …) emit it inline when the server does not separate it.
//!
//! Only a tag at the very start of the reply (after whitespace) opens a thinking
//! section, so code in the answer that mentions `<think>` is left alone. Tags may be
//! split across chunks.

const OPEN: &str = "<think>";
const CLOSE: &str = "</think>";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Piece {
    Text(String),
    Thinking(String),
    ThinkEnd,
}

#[derive(Default)]
pub struct ThinkSplitter {
    in_think: bool,
    /// Non-whitespace text has been emitted; an opening tag is no longer recognised.
    answered: bool,
    /// Drop whitespace right after `</think>`.
    skip_whitespace: bool,
    pending: String,
}

impl ThinkSplitter {
    pub fn push(&mut self, chunk: &str) -> Vec<Piece> {
        let mut buf = std::mem::take(&mut self.pending);
        buf.push_str(chunk);
        let mut out = Vec::new();
        loop {
            if self.in_think {
                if let Some(at) = buf.find(CLOSE) {
                    emit(&mut out, Piece::Thinking(buf[..at].to_owned()));
                    out.push(Piece::ThinkEnd);
                    self.in_think = false;
                    self.answered = true;
                    // Models usually put a newline or two after the closing tag; they
                    // may arrive in later chunks.
                    self.skip_whitespace = true;
                    buf = buf[at + CLOSE.len()..].to_owned();
                    continue;
                }
                let keep = partial_suffix(&buf, CLOSE);
                emit(
                    &mut out,
                    Piece::Thinking(buf[..buf.len() - keep].to_owned()),
                );
                self.pending = buf[buf.len() - keep..].to_owned();
                return out;
            }
            if self.skip_whitespace {
                let trimmed = buf.trim_start();
                if trimmed.is_empty() {
                    return out;
                }
                buf = trimmed.to_owned();
                self.skip_whitespace = false;
            }
            if !self.answered {
                let trimmed = buf.trim_start();
                if let Some(rest) = trimmed.strip_prefix(OPEN) {
                    buf = rest.to_owned();
                    self.in_think = true;
                    continue;
                }
                if trimmed.is_empty() || OPEN.starts_with(trimmed) {
                    // Could still become `<think>`: wait for more input.
                    self.pending = buf;
                    return out;
                }
                self.answered = true;
            }
            emit(&mut out, Piece::Text(std::mem::take(&mut buf)));
            return out;
        }
    }

    /// Flushes whatever is held back at the end of the stream.
    pub fn finish(&mut self) -> Vec<Piece> {
        let rest = std::mem::take(&mut self.pending);
        let mut out = Vec::new();
        if self.in_think {
            emit(&mut out, Piece::Thinking(rest));
            out.push(Piece::ThinkEnd);
            self.in_think = false;
        } else {
            emit(&mut out, Piece::Text(rest));
        }
        out
    }
}

fn emit(out: &mut Vec<Piece>, piece: Piece) {
    let empty = matches!(&piece, Piece::Text(s) | Piece::Thinking(s) if s.is_empty());
    if !empty {
        out.push(piece);
    }
}

/// Length of the longest suffix of `s` that is a proper prefix of `tag`.
fn partial_suffix(s: &str, tag: &str) -> usize {
    (1..tag.len())
        .rev()
        .find(|&n| {
            s.len() >= n && s.is_char_boundary(s.len() - n) && tag.starts_with(&s[s.len() - n..])
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(chunks: &[&str]) -> (String, String, usize) {
        let mut s = ThinkSplitter::default();
        let mut pieces: Vec<Piece> = chunks.iter().flat_map(|c| s.push(c)).collect();
        pieces.extend(s.finish());
        let (mut text, mut thinking, mut ends) = (String::new(), String::new(), 0);
        for p in pieces {
            match p {
                Piece::Text(t) => text.push_str(&t),
                Piece::Thinking(t) => thinking.push_str(&t),
                Piece::ThinkEnd => ends += 1,
            }
        }
        (text, thinking, ends)
    }

    const REPLY: &str =
        "\n<think>Need a blue button.\nOK.</think>\n\nDone: `<think>` stays in code.";

    #[test]
    fn splits_whole_reply() {
        let (text, thinking, ends) = run(&[REPLY]);
        assert_eq!(thinking, "Need a blue button.\nOK.");
        assert_eq!(text, "Done: `<think>` stays in code.");
        assert_eq!(ends, 1);
    }

    #[test]
    fn any_chunking_gives_same_result() {
        let chars: Vec<char> = REPLY.chars().collect();
        for size in 1..=8 {
            let chunks: Vec<String> = chars.chunks(size).map(|c| c.iter().collect()).collect();
            let refs: Vec<&str> = chunks.iter().map(String::as_str).collect();
            assert_eq!(run(&refs), run(&[REPLY]), "chunk size {size}");
        }
    }

    #[test]
    fn plain_text_and_late_tags_pass_through() {
        assert_eq!(
            run(&["Hello ", "<think>x</think>"]),
            ("Hello <think>x</think>".into(), String::new(), 0)
        );
        assert_eq!(
            run(&["<thi", "s is not a tag"]),
            ("<this is not a tag".into(), String::new(), 0)
        );
    }

    #[test]
    fn unterminated_thinking_is_flushed() {
        assert_eq!(
            run(&["<think>still going"]),
            (String::new(), "still going".into(), 1)
        );
    }

    #[test]
    fn multibyte_text_is_safe() {
        assert_eq!(
            run(&["<think>想一下</think>好的"]),
            ("好的".into(), "想一下".into(), 1)
        );
    }
}
