//! Server-sent events decoding, shared by every streaming provider.

use std::collections::VecDeque;

use futures_util::StreamExt;
use futures_util::stream::{self, BoxStream};

use crate::{ByteStream, ProviderError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    /// The `event:` field, if any.
    pub event: Option<String>,
    /// `data:` lines joined with `\n`.
    pub data: String,
}

/// Incremental SSE decoder. Accepts arbitrary chunk boundaries, including ones
/// that split a UTF-8 character or a `\r\n` pair.
#[derive(Default)]
pub struct SseDecoder {
    buf: Vec<u8>,
    event: Option<String>,
    data: Vec<String>,
}

impl SseDecoder {
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<SseEvent>, ProviderError> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        while let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
            let mut line: Vec<u8> = self.buf.drain(..=pos).collect();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            let line = String::from_utf8(line).map_err(|e| ProviderError::Decode(e.to_string()))?;
            if let Some(ev) = self.line(&line) {
                out.push(ev);
            }
        }
        Ok(out)
    }

    /// Flushes a final event that lacks its terminating blank line.
    pub fn finish(&mut self) -> Result<Vec<SseEvent>, ProviderError> {
        let mut out = self.push(b"\n")?;
        out.extend(self.dispatch());
        Ok(out)
    }

    fn line(&mut self, line: &str) -> Option<SseEvent> {
        if line.is_empty() {
            return self.dispatch();
        }
        if line.starts_with(':') {
            return None;
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "event" => self.event = Some(value.to_owned()),
            "data" => self.data.push(value.to_owned()),
            _ => {} // `id`, `retry` and unknown fields are not used by providers.
        }
        None
    }

    fn dispatch(&mut self) -> Option<SseEvent> {
        if self.data.is_empty() && self.event.is_none() {
            return None;
        }
        Some(SseEvent {
            event: self.event.take(),
            data: std::mem::take(&mut self.data).join("\n"),
        })
    }
}

/// Decodes an HTTP body into SSE events.
pub fn sse_events(body: ByteStream) -> BoxStream<'static, Result<SseEvent, ProviderError>> {
    struct State {
        body: ByteStream,
        decoder: SseDecoder,
        queue: VecDeque<SseEvent>,
        done: bool,
    }
    let state = State {
        body,
        decoder: SseDecoder::default(),
        queue: VecDeque::new(),
        done: false,
    };
    stream::unfold(state, |mut s| async move {
        loop {
            if let Some(ev) = s.queue.pop_front() {
                return Some((Ok(ev), s));
            }
            if s.done {
                return None;
            }
            let decoded = match s.body.next().await {
                Some(Ok(chunk)) => s.decoder.push(&chunk),
                Some(Err(e)) => Err(e),
                None => {
                    s.done = true;
                    s.decoder.finish()
                }
            };
            match decoded {
                Ok(events) => s.queue.extend(events),
                Err(e) => {
                    s.done = true;
                    return Some((Err(e), s));
                }
            }
        }
    })
    .boxed()
}

#[cfg(test)]
mod tests {
    use futures_util::TryStreamExt;

    use super::*;

    const SAMPLE: &str = ": keep-alive\n\
        event: message_start\n\
        data: {\"a\":1}\n\
        \n\
        data: line one\r\n\
        data: line two\r\n\
        \r\n\
        event: delta\n\
        data: {\"text\":\"你好\"}\n\
        \n";

    fn expected() -> Vec<SseEvent> {
        vec![
            SseEvent {
                event: Some("message_start".into()),
                data: "{\"a\":1}".into(),
            },
            SseEvent {
                event: None,
                data: "line one\nline two".into(),
            },
            SseEvent {
                event: Some("delta".into()),
                data: "{\"text\":\"你好\"}".into(),
            },
        ]
    }

    #[test]
    fn decodes_whole_body() {
        let mut d = SseDecoder::default();
        let mut got = d.push(SAMPLE.as_bytes()).unwrap();
        got.extend(d.finish().unwrap());
        assert_eq!(got, expected());
    }

    #[test]
    fn any_chunking_gives_same_events() {
        let bytes = SAMPLE.as_bytes();
        for size in 1..=bytes.len() {
            let mut d = SseDecoder::default();
            let mut got = Vec::new();
            for chunk in bytes.chunks(size) {
                got.extend(d.push(chunk).unwrap());
            }
            got.extend(d.finish().unwrap());
            assert_eq!(got, expected(), "chunk size {size}");
        }
    }

    #[test]
    fn final_event_without_blank_line_is_flushed() {
        let mut d = SseDecoder::default();
        assert!(d.push(b"data: [DONE]").unwrap().is_empty());
        assert_eq!(d.finish().unwrap()[0].data, "[DONE]");
    }

    #[test]
    fn invalid_utf8_is_a_decode_error() {
        let mut d = SseDecoder::default();
        assert!(matches!(
            d.push(b"data: \xff\n"),
            Err(ProviderError::Decode(_))
        ));
    }

    #[tokio::test]
    async fn stream_adapter_matches_decoder() {
        let chunks: Vec<Result<Vec<u8>, ProviderError>> = SAMPLE
            .as_bytes()
            .chunks(7)
            .map(|c| Ok(c.to_vec()))
            .collect();
        let got: Vec<SseEvent> = sse_events(stream::iter(chunks).boxed())
            .try_collect()
            .await
            .unwrap();
        assert_eq!(got, expected());
    }
}
