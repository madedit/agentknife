//! Real network transport (reqwest + rustls).

use std::time::Duration;

use futures_util::StreamExt;
use futures_util::future::BoxFuture;

use crate::{HttpRequest, HttpResponse, HttpTransport, Method, ProviderError};

pub struct ReqwestTransport {
    client: reqwest::Client,
}

impl ReqwestTransport {
    pub fn new() -> Result<Self, ProviderError> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(30))
            // No total timeout: streamed turns can run for many minutes. A stalled
            // stream (no bytes, not even pings) is cut after this long.
            .read_timeout(Duration::from_secs(300))
            .build()
            .map_err(|e| ProviderError::Transport(e.to_string()))?;
        Ok(Self { client })
    }
}

impl HttpTransport for ReqwestTransport {
    fn send(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, ProviderError>> {
        Box::pin(async move {
            let mut builder = match request.method {
                Method::Get => self.client.get(&request.url),
                Method::Post => self.client.post(&request.url),
            };
            for (k, v) in &request.headers {
                builder = builder.header(k, v);
            }
            if let Some(body) = request.body {
                builder = builder.body(body);
            }
            let resp = builder
                .send()
                .await
                .map_err(|e| ProviderError::Network(e.without_url().to_string()))?;
            let status = resp.status().as_u16();
            let headers = resp
                .headers()
                .iter()
                .filter_map(|(k, v)| Some((k.as_str().to_owned(), v.to_str().ok()?.to_owned())))
                .collect();
            let body = resp
                .bytes_stream()
                .map(|chunk| {
                    chunk
                        .map(|b| b.to_vec())
                        .map_err(|e| ProviderError::Network(e.without_url().to_string()))
                })
                .boxed();
            Ok(HttpResponse {
                status,
                headers,
                body,
            })
        })
    }
}
