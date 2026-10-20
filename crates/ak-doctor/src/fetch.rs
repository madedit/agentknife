//! HTTP downloads with resume. Tests replace it with in-memory files.

use std::time::Duration;

use futures_util::StreamExt;
use futures_util::future::BoxFuture;
use futures_util::stream::BoxStream;

pub struct Download {
    /// The server honoured the Range request: the body continues at `from`.
    /// Otherwise it starts at byte 0.
    pub resumed: bool,
    /// Size of the whole file, when the server says.
    pub size: Option<u64>,
    pub body: BoxStream<'static, Result<Vec<u8>, String>>,
}

pub trait Fetch: Send + Sync {
    /// GETs `url`, asking for the bytes from `from` on when it is not 0.
    fn get<'a>(&'a self, url: &'a str, from: u64) -> BoxFuture<'a, Result<Download, String>>;
}

/// The real network.
pub struct HttpFetch {
    client: reqwest::Client,
}

impl HttpFetch {
    pub fn new() -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .user_agent(concat!("AgentKnife/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(30))
            .read_timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self { client })
    }
}

impl Fetch for HttpFetch {
    fn get<'a>(&'a self, url: &'a str, from: u64) -> BoxFuture<'a, Result<Download, String>> {
        Box::pin(async move {
            let mut req = self.client.get(url);
            if from > 0 {
                req = req.header(reqwest::header::RANGE, format!("bytes={from}-"));
            }
            let resp = req.send().await.map_err(|e| format!("{url}: {e}"))?;
            let status = resp.status();
            if from > 0 && status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
                // The partial file is already whole (or bigger): start over.
                return self.get(url, 0).await;
            }
            if !status.is_success() {
                return Err(format!("{url}: HTTP {status}"));
            }
            let resumed = status == reqwest::StatusCode::PARTIAL_CONTENT;
            let size = resp
                .content_length()
                .map(|n| if resumed { n + from } else { n });
            let body = resp
                .bytes_stream()
                .map(|chunk| chunk.map(|b| b.to_vec()).map_err(|e| e.to_string()))
                .boxed();
            Ok(Download {
                resumed,
                size,
                body,
            })
        })
    }
}
