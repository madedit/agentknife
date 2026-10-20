//! Fixture replay: providers are tested against recorded HTTP exchanges and never
//! call real APIs (AGENTS.md §7).
//!
//! Fixtures live in `crates/ak-providers/tests/fixtures/<provider>/<name>.json`.
//! A fixture never stores credentials: the request side records only method, path
//! and (optionally) the JSON body to assert on.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use futures_util::future::BoxFuture;
use futures_util::stream;
use serde::{Deserialize, Serialize};

use crate::{HttpRequest, HttpResponse, HttpTransport, Method, ProviderError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fixture {
    /// `"recorded"` (captured from the real API) or `"synthetic"` (hand-written from
    /// the documented wire format; replace by recording when a key is available).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub request: RecordedRequest,
    pub response: RecordedResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordedRequest {
    /// `"GET"` or `"POST"`.
    pub method: String,
    /// URL path (and query), without scheme and host.
    pub path: String,
    /// When present, the sent body must equal this JSON.
    #[serde(default)]
    pub body: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordedResponse {
    pub status: u16,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    /// Body chunks as received. Joined, they form the full body.
    pub chunks: Vec<String>,
}

impl Fixture {
    pub fn load(path: impl AsRef<Path>) -> Result<Vec<Fixture>, ProviderError> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path)
            .map_err(|e| ProviderError::Transport(format!("{}: {e}", path.display())))?;
        // A file holds either one exchange or a list of them (multi-request flows).
        serde_json::from_str::<Vec<Fixture>>(&text)
            .or_else(|_| serde_json::from_str::<Fixture>(&text).map(|f| vec![f]))
            .map_err(|e| ProviderError::Transport(format!("{}: {e}", path.display())))
    }
}

/// Replays fixtures in order and checks each request against its recording.
pub struct FixtureTransport {
    queue: Mutex<VecDeque<Fixture>>,
    rechunk: Option<usize>,
    sent: Mutex<Vec<HttpRequest>>,
}

impl FixtureTransport {
    pub fn new(fixtures: Vec<Fixture>) -> Self {
        Self {
            queue: Mutex::new(fixtures.into()),
            rechunk: None,
            sent: Mutex::new(Vec::new()),
        }
    }

    /// Re-splits every body into `size`-byte chunks, to prove decoders do not
    /// depend on network chunk boundaries.
    pub fn rechunked(mut self, size: usize) -> Self {
        self.rechunk = Some(size.max(1));
        self
    }

    /// Requests sent so far, for assertions.
    pub fn sent(&self) -> Vec<HttpRequest> {
        self.sent.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Fixtures not yet consumed. Tests should assert this is zero.
    pub fn remaining(&self) -> usize {
        self.queue.lock().map(|q| q.len()).unwrap_or_default()
    }

    fn replay(&self, request: HttpRequest) -> Result<HttpResponse, ProviderError> {
        fn poisoned<T>(_: T) -> ProviderError {
            ProviderError::Transport("fixture lock poisoned".into())
        }
        self.sent.lock().map_err(poisoned)?.push(request.clone());
        let fixture = self
            .queue
            .lock()
            .map_err(poisoned)?
            .pop_front()
            .ok_or_else(|| ProviderError::Transport(format!("no fixture left for {request:?}")))?;
        check_request(&fixture.request, &request)?;

        let body: Vec<u8> = fixture.response.chunks.concat().into_bytes();
        let chunks: Vec<Vec<u8>> = match self.rechunk {
            Some(size) => body.chunks(size).map(<[u8]>::to_vec).collect(),
            None => fixture
                .response
                .chunks
                .iter()
                .map(|c| c.as_bytes().to_vec())
                .collect(),
        };
        Ok(HttpResponse {
            status: fixture.response.status,
            headers: fixture.response.headers,
            body: stream::iter(chunks.into_iter().map(Ok)).boxed(),
        })
    }
}

impl HttpTransport for FixtureTransport {
    fn send(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, ProviderError>> {
        let result = self.replay(request);
        Box::pin(async move { result })
    }
}

/// Response headers worth keeping in a fixture. Everything else (cookies, org ids,
/// rate-limit details) is dropped.
const KEPT_HEADERS: &[&str] = &["content-type", "retry-after", "request-id"];

/// Wraps a real transport and records every exchange as a [`Fixture`]. Request
/// headers (credentials) are never recorded.
pub struct RecordingTransport<T> {
    inner: T,
    recorded: Arc<Mutex<Vec<Option<Fixture>>>>,
}

impl<T: HttpTransport> RecordingTransport<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            recorded: Arc::default(),
        }
    }

    /// Completed exchanges in request order. A response is complete once its body
    /// has been read to the end.
    pub fn take(&self) -> Vec<Fixture> {
        self.recorded
            .lock()
            .map(|mut r| r.drain(..).flatten().collect())
            .unwrap_or_default()
    }
}

impl<T: HttpTransport> HttpTransport for RecordingTransport<T> {
    fn send(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, ProviderError>> {
        Box::pin(async move {
            let recorded_request = RecordedRequest {
                method: match request.method {
                    Method::Get => "GET".into(),
                    Method::Post => "POST".into(),
                },
                path: path_of(&request.url).to_owned(),
                body: request
                    .body
                    .as_deref()
                    .and_then(|b| serde_json::from_slice(b).ok()),
            };
            let resp = self.inner.send(request).await?;
            let slot = match self.recorded.lock() {
                Ok(mut r) => {
                    r.push(None);
                    r.len() - 1
                }
                Err(_) => return Err(ProviderError::Transport("recorder lock poisoned".into())),
            };
            let headers: Vec<(String, String)> = resp
                .headers
                .iter()
                .filter(|(k, _)| KEPT_HEADERS.iter().any(|h| k.eq_ignore_ascii_case(h)))
                .cloned()
                .collect();
            let status = resp.status;
            let buf: Arc<Mutex<Vec<u8>>> = Arc::default();
            let tee = buf.clone();
            let recorded = self.recorded.clone();
            let kept = headers.clone();
            let body = resp.body.inspect(move |chunk| {
                if let (Ok(c), Ok(mut b)) = (chunk, tee.lock()) {
                    b.extend_from_slice(c);
                }
            });
            let finish = stream::once(async move {
                let bytes = buf
                    .lock()
                    .map(|mut b| std::mem::take(&mut *b))
                    .unwrap_or_default();
                let fixture = Fixture {
                    source: Some("recorded".into()),
                    request: recorded_request,
                    response: RecordedResponse {
                        status,
                        headers: kept,
                        chunks: vec![String::from_utf8_lossy(&bytes).into_owned()],
                    },
                };
                if let Ok(mut r) = recorded.lock()
                    && let Some(s) = r.get_mut(slot)
                {
                    *s = Some(fixture);
                }
                None::<Result<Vec<u8>, ProviderError>>
            })
            .filter_map(|x| async move { x });
            Ok(HttpResponse {
                status,
                headers,
                body: body.chain(finish).boxed(),
            })
        })
    }
}

fn check_request(recorded: &RecordedRequest, actual: &HttpRequest) -> Result<(), ProviderError> {
    let method = match actual.method {
        Method::Get => "GET",
        Method::Post => "POST",
    };
    let path = path_of(&actual.url);
    if !recorded.method.eq_ignore_ascii_case(method) || recorded.path != path {
        return Err(ProviderError::Transport(format!(
            "fixture expects {} {}, got {method} {path}",
            recorded.method, recorded.path
        )));
    }
    if let Some(expected) = &recorded.body {
        let sent: serde_json::Value = actual
            .body
            .as_deref()
            .map(serde_json::from_slice)
            .transpose()
            .map_err(|e| ProviderError::Transport(format!("request body is not JSON: {e}")))?
            .unwrap_or(serde_json::Value::Null);
        if &sent != expected {
            return Err(ProviderError::Transport(format!(
                "request body differs from fixture:\n  expected {expected}\n  got      {sent}"
            )));
        }
    }
    Ok(())
}

/// `https://host:1234/v1/x?y=1` -> `/v1/x?y=1`
fn path_of(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.find('/').map_or("/", |i| &rest[i..])
}

#[cfg(test)]
mod tests {
    use futures_util::TryStreamExt;

    use super::*;
    use crate::sse::{SseEvent, sse_events};

    fn fixture_path() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/harness/sse_basic.json")
    }

    fn request(path: &str, body: &str) -> HttpRequest {
        HttpRequest {
            method: Method::Post,
            url: format!("https://api.example.com{path}"),
            headers: vec![("x-api-key".into(), "sk-test".into())],
            body: Some(body.as_bytes().to_vec()),
        }
    }

    async fn events(transport: &FixtureTransport) -> Vec<SseEvent> {
        let resp = transport
            .send(request("/v1/chat", r#"{"model":"m","stream":true}"#))
            .await
            .unwrap();
        assert_eq!(resp.status, 200);
        sse_events(resp.body).try_collect().await.unwrap()
    }

    #[tokio::test]
    async fn replays_fixture_identically_at_any_chunk_size() {
        let fixtures = Fixture::load(fixture_path()).unwrap();
        let baseline = events(&FixtureTransport::new(fixtures.clone())).await;
        assert_eq!(baseline.len(), 3);
        assert_eq!(baseline[1].data, r#"{"text":"你好"}"#);
        for size in [1, 2, 3, 5, 64] {
            let t = FixtureTransport::new(fixtures.clone()).rechunked(size);
            assert_eq!(events(&t).await, baseline, "chunk size {size}");
            assert_eq!(t.remaining(), 0);
        }
    }

    #[tokio::test]
    async fn mismatched_request_is_rejected() {
        let t = FixtureTransport::new(Fixture::load(fixture_path()).unwrap());
        let wrong_body = t.send(request("/v1/chat", r#"{"model":"other"}"#)).await;
        assert!(matches!(wrong_body, Err(ProviderError::Transport(_))));

        let t = FixtureTransport::new(Fixture::load(fixture_path()).unwrap());
        let wrong_path = t.send(request("/v2/chat", "{}")).await;
        assert!(matches!(wrong_path, Err(ProviderError::Transport(_))));
    }

    #[tokio::test]
    async fn exhausted_fixtures_fail() {
        let t = FixtureTransport::new(Vec::new());
        assert!(t.send(request("/v1/chat", "{}")).await.is_err());
        assert_eq!(t.sent().len(), 1);
    }

    #[test]
    fn path_extraction() {
        assert_eq!(path_of("https://h:1/v1/x?y=1"), "/v1/x?y=1");
        assert_eq!(path_of("http://localhost:11434"), "/");
    }
}
