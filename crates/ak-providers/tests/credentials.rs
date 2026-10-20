//! API keys stay out of settings files, logs and error messages (P1-PROV-004).
//! Uses the real HTTP transport against a local server that echoes the key.

// Test code: clippy's allow-unwrap-in-tests does not cover helper fns here.
#![allow(clippy::unwrap_used)]

use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use ak_providers::anthropic::{Anthropic, AnthropicConfig};
use ak_providers::credentials::{ApiKey, CredentialStore, MemoryStore, account};
use ak_providers::openai::{OpenAiCompat, OpenAiCompatConfig};
use ak_providers::{ChatRequest, Provider, ProviderError, ReqwestTransport};
use ak_types::chat::Message;
use ak_types::settings::{AiConnection, AiOverride};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};

const KEY: &str = "sk-ant-api03-TOPSECRET-0123456789abcdef";

/// Records every tracing event and span field, at every level.
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<String>>);

struct Fields<'a>(&'a mut String);

impl Visit for Fields<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let _ = write!(self.0, " {}={value:?}", field.name());
    }
}

impl Subscriber for Capture {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, span: &Attributes<'_>) -> Id {
        span.record(&mut Fields(&mut self.0.lock().unwrap()));
        Id::from_u64(1)
    }
    fn record(&self, _: &Id, values: &Record<'_>) {
        values.record(&mut Fields(&mut self.0.lock().unwrap()));
    }
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event(&self, event: &Event<'_>) {
        let mut out = self.0.lock().unwrap();
        out.push('\n');
        event.record(&mut Fields(&mut out));
    }
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}

/// Answers each connection with `status` and a body that echoes the key, the
/// way some servers do. Returns the base URL and the raw requests received.
fn echo_server(status: &'static str, connections: usize) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().take(connections) {
            let mut stream = stream.unwrap();
            let mut buf = vec![0; 64 * 1024];
            let n = stream.read(&mut buf).unwrap();
            log.lock()
                .unwrap()
                .push(String::from_utf8_lossy(&buf[..n]).into_owned());
            let body = format!(
                r#"{{"error":{{"type":"invalid_request_error","message":"Bad key {KEY} for this model"}}}}"#
            );
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    (url, seen)
}

fn request() -> ChatRequest {
    ChatRequest {
        model: "m".into(),
        system: None,
        messages: vec![Message::user_text("hi")],
        tools: vec![],
        max_tokens: 100,
        effort: None,
    }
}

fn assert_clean(what: &str, text: &str) {
    assert!(!text.contains(KEY), "{what} leaks the key:\n{text}");
    assert!(
        !text.contains("TOPSECRET"),
        "{what} leaks part of the key:\n{text}"
    );
}

#[tokio::test]
async fn keys_never_reach_settings_logs_or_errors() {
    let capture = Capture::default();
    let _guard = tracing::subscriber::set_default(capture.clone());

    // The key lives in the store under the connection's account, nowhere else.
    let (anthropic_url, seen) = echo_server("400 Bad Request", 1);
    let conn = AiConnection {
        provider: "anthropic".into(),
        base_url: Some(anthropic_url.clone()),
        model: "m".into(),
        effort: None,
    };
    let store = MemoryStore::default();
    store
        .set(&account(&conn), &ApiKey::new(KEY).unwrap())
        .unwrap();
    let settings = serde_json::to_string(&(&conn, AiOverride::default())).unwrap();
    assert_clean("serialized settings", &settings);

    let key = store.get(&account(&conn)).unwrap().unwrap();
    let mut config = AnthropicConfig::new(key);
    config.base_url = anthropic_url;
    assert_clean("AnthropicConfig Debug", &format!("{config:?}"));
    let provider = Anthropic::new(config, Arc::new(ReqwestTransport::new().unwrap()));
    let err = provider.stream(request()).await.err().unwrap();
    assert!(matches!(err, ProviderError::InvalidRequest(_)), "{err:?}");
    assert!(err.to_string().contains("<redacted>"), "{err}");
    assert_clean("error Display", &err.to_string());
    assert_clean("error Debug", &format!("{err:?}"));
    // Sanity: the key did go to the server, in the header only.
    let sent = seen.lock().unwrap().join("\n");
    assert!(
        sent.to_ascii_lowercase()
            .contains(&format!("x-api-key: {}", KEY.to_ascii_lowercase()))
    );

    // Same for the OpenAI-compatible provider (Bearer header).
    let (openai_url, _) = echo_server("500 Internal Server Error", 1);
    let config = OpenAiCompatConfig::new(format!("{openai_url}/v1"), Some(KEY.into()));
    assert_clean("OpenAiCompatConfig Debug", &format!("{config:?}"));
    let provider = OpenAiCompat::new(config, Arc::new(ReqwestTransport::new().unwrap()));
    let err = provider.stream(request()).await.err().unwrap();
    assert!(
        matches!(err, ProviderError::Server { status: 500, .. }),
        "{err:?}"
    );
    assert_clean("error Display", &err.to_string());

    let logs = capture.0.lock().unwrap().clone();
    assert!(
        logs.contains("provider request"),
        "the request was logged:\n{logs}"
    );
    assert!(logs.contains("<redacted>"), "{logs}");
    assert_clean("tracing output", &logs);
}

/// Writes, reads and deletes a throwaway entry in the real OS keychain.
/// `cargo test -p ak-providers --test credentials -- --ignored`
#[test]
#[ignore = "touches the real OS keychain"]
fn real_keychain_round_trip() {
    use ak_providers::credentials::Keychain;
    let account = "agentknife-selftest";
    let key = ApiKey::new("sk-selftest-not-a-real-key").unwrap();
    Keychain.set(account, &key).unwrap();
    assert_eq!(Keychain.get(account).unwrap(), Some(key));
    Keychain.delete(account).unwrap();
    assert_eq!(Keychain.get(account).unwrap(), None);
    Keychain.delete(account).unwrap();
}
