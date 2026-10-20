use std::fmt;

use futures_util::future::BoxFuture;
use futures_util::stream::BoxStream;

use crate::ProviderError;

/// Header names whose values never appear in logs or `Debug` output.
const SECRET_HEADERS: &[&str] = &["authorization", "x-api-key", "api-key", "x-goog-api-key"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

#[derive(Clone)]
pub struct HttpRequest {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

impl fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let headers: Vec<(&str, &str)> = self
            .headers
            .iter()
            .map(|(k, v)| {
                let secret = SECRET_HEADERS.iter().any(|s| k.eq_ignore_ascii_case(s));
                (k.as_str(), if secret { "<redacted>" } else { v.as_str() })
            })
            .collect();
        f.debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("url", &self.url)
            .field("headers", &headers)
            .field("body_len", &self.body.as_ref().map(Vec::len))
            .finish()
    }
}

pub type ByteStream = BoxStream<'static, Result<Vec<u8>, ProviderError>>;

pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: ByteStream,
}

impl HttpResponse {
    /// Reads the whole body as UTF-8 (error bodies, JSON endpoints).
    pub async fn text(mut self) -> Result<String, ProviderError> {
        use futures_util::StreamExt;
        let mut bytes = Vec::new();
        while let Some(chunk) = self.body.next().await {
            bytes.extend(chunk?);
        }
        String::from_utf8(bytes).map_err(|e| ProviderError::Decode(e.to_string()))
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// Passes 2xx responses through; turns anything else into a typed error, using the
/// `{"error": {"message": …}}` body most providers send.
pub(crate) async fn ok_or_error(resp: HttpResponse) -> Result<HttpResponse, ProviderError> {
    if (200..300).contains(&resp.status) {
        return Ok(resp);
    }
    let status = resp.status;
    let retry_after = resp.header("retry-after").map(str::to_owned);
    let text = resp.text().await.unwrap_or_default();
    let message = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_owned))
        .unwrap_or(text);
    Err(ProviderError::from_status(
        status,
        retry_after.as_deref(),
        &message,
    ))
}

/// The only way providers reach the network. Real HTTP and fixture replay both
/// implement it.
pub trait HttpTransport: Send + Sync {
    fn send(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, ProviderError>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_secrets() {
        let req = HttpRequest {
            method: Method::Post,
            url: "https://api.example.com/v1/messages".into(),
            headers: vec![
                ("x-api-key".into(), "sk-secret-123".into()),
                ("Authorization".into(), "Bearer sk-secret-456".into()),
                ("content-type".into(), "application/json".into()),
            ],
            body: Some(b"{}".to_vec()),
        };
        let shown = format!("{req:?}");
        assert!(!shown.contains("sk-secret"), "{shown}");
        assert!(shown.contains("application/json"));
    }
}
