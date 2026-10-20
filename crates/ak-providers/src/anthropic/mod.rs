//! Anthropic Messages API provider (raw HTTP + SSE; Rust has no official SDK).
//!
//! Behaviour follows the current API (2026-10): adaptive thinking with summarized
//! display and `output_config.effort`, verbatim replay of our own content blocks,
//! `thinking.block_binding` drop-instead-of-400 as a safety net for history edits,
//! eager tool-input streaming, automatic prompt caching, and server-side refusal
//! fallbacks on the Claude API. Per-model choices come from `GET /v1/models/{id}`.

mod request;
mod stream;

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use futures_util::future::BoxFuture;
use futures_util::stream as fstream;
use serde_json::Value;

use crate::credentials::ApiKey;
use crate::sse::sse_events;
use crate::transport::ok_or_error;
use crate::{
    Capabilities, ChatRequest, Effort, EventStream, HttpRequest, HttpResponse, HttpTransport,
    Method, ModelInfo, Provider, ProviderError,
};

/// Provider id ([`ak_types::chat::ProviderReplay::origin`]).
pub const ID: &str = "anthropic";
pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
pub const API_VERSION: &str = "2023-06-01";
/// Suggested in the connect wizard.
pub const DEFAULT_MODEL: &str = "claude-opus-5-5";

#[derive(Clone)]
pub struct AnthropicConfig {
    pub api_key: ApiKey,
    /// Override for proxies or Claude Platform on AWS. Server-side fallbacks are only
    /// requested against [`DEFAULT_BASE_URL`].
    pub base_url: String,
}

impl AnthropicConfig {
    pub fn new(api_key: impl Into<ApiKey>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: DEFAULT_BASE_URL.into(),
        }
    }
}

impl fmt::Debug for AnthropicConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnthropicConfig")
            .field("api_key", &"<redacted>")
            .field("base_url", &self.base_url)
            .finish()
    }
}

/// What we need to know about a model to build a valid request.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelMeta {
    pub max_input_tokens: Option<u32>,
    pub max_tokens: Option<u32>,
    pub vision: bool,
    pub thinking: bool,
    pub adaptive_thinking: bool,
    /// Only `{type: "enabled", budget_tokens}` (pre-adaptive models).
    pub budget_thinking: bool,
    /// Supported `output_config.effort` levels; empty when effort is unsupported.
    pub effort_levels: Vec<Effort>,
}

impl ModelMeta {
    /// Parses a `/v1/models/{id}` object.
    pub fn from_json(v: &Value) -> Self {
        let caps = &v["capabilities"];
        let on = |path: &str| caps.pointer(path).and_then(Value::as_bool).unwrap_or(false);
        let adaptive = on("/thinking/types/adaptive/supported");
        let effort_levels = if on("/effort/supported") {
            Effort::ALL
                .into_iter()
                .filter(|e| on(&format!("/effort/{}/supported", e.as_str())))
                .collect()
        } else {
            Vec::new()
        };
        let tokens = |field: &str| v[field].as_u64().and_then(|n| u32::try_from(n).ok());
        Self {
            max_input_tokens: tokens("max_input_tokens"),
            max_tokens: tokens("max_tokens"),
            vision: on("/image_input/supported"),
            thinking: on("/thinking/supported"),
            adaptive_thinking: adaptive,
            budget_thinking: !adaptive && on("/thinking/types/enabled/supported"),
            effort_levels,
        }
    }

    /// The highest supported effort not above `wanted`.
    pub fn effort_at_most(&self, wanted: Effort) -> Option<Effort> {
        self.effort_levels
            .iter()
            .copied()
            .filter(|e| *e <= wanted)
            .max()
    }
}

pub struct Anthropic {
    config: AnthropicConfig,
    transport: Arc<dyn HttpTransport>,
    models: Mutex<HashMap<String, ModelMeta>>,
}

impl Anthropic {
    pub fn new(config: AnthropicConfig, transport: Arc<dyn HttpTransport>) -> Self {
        Self {
            config,
            transport,
            models: Mutex::new(HashMap::new()),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.config.base_url.trim_end_matches('/'))
    }

    fn uses_claude_api(&self) -> bool {
        self.config.base_url.trim_end_matches('/') == DEFAULT_BASE_URL
    }

    fn headers(&self, betas: &[&str]) -> Vec<(String, String)> {
        let mut h = vec![
            (
                "x-api-key".to_owned(),
                self.config.api_key.expose().to_owned(),
            ),
            ("anthropic-version".to_owned(), API_VERSION.to_owned()),
            ("content-type".to_owned(), "application/json".to_owned()),
        ];
        if !betas.is_empty() {
            h.push(("anthropic-beta".to_owned(), betas.join(",")));
        }
        h
    }

    async fn get_json(&self, path: &str) -> Result<Value, ProviderError> {
        let resp = self
            .transport
            .send(HttpRequest {
                method: Method::Get,
                url: self.url(path),
                headers: self.headers(&[]),
                body: None,
            })
            .await?;
        let resp = ok_or_error(resp, Some(&self.config.api_key)).await?;
        let text = resp.text().await?;
        serde_json::from_str(&text).map_err(|e| ProviderError::Decode(e.to_string()))
    }

    /// Model limits and capabilities, fetched once per model and cached.
    pub async fn model_meta(&self, model: &str) -> Result<ModelMeta, ProviderError> {
        if let Some(m) = self.models.lock().ok().and_then(|m| m.get(model).cloned()) {
            return Ok(m);
        }
        let meta = match self.get_json(&format!("/v1/models/{model}")).await {
            Ok(v) => ModelMeta::from_json(&v),
            // Compatible servers and proxies often lack the models endpoint. Without
            // it, send no model-specific options (thinking, effort, output clamp).
            Err(ProviderError::ModelNotFound(_)) if !self.uses_claude_api() => ModelMeta::default(),
            Err(e) => return Err(e),
        };
        if let Ok(mut cache) = self.models.lock() {
            cache.insert(model.to_owned(), meta.clone());
        }
        Ok(meta)
    }
}

impl Provider for Anthropic {
    fn id(&self) -> &'static str {
        ID
    }

    fn list_models(&self) -> BoxFuture<'_, Result<Vec<ModelInfo>, ProviderError>> {
        Box::pin(async move {
            let mut models = Vec::new();
            let mut after: Option<String> = None;
            loop {
                let path = match &after {
                    Some(id) => format!("/v1/models?limit=1000&after_id={id}"),
                    None => "/v1/models?limit=1000".to_owned(),
                };
                let page = self.get_json(&path).await?;
                for m in page["data"].as_array().into_iter().flatten() {
                    if let Some(id) = m["id"].as_str() {
                        models.push(ModelInfo {
                            id: id.to_owned(),
                            display_name: m["display_name"].as_str().map(str::to_owned),
                        });
                    }
                }
                match (page["has_more"].as_bool(), page["last_id"].as_str()) {
                    (Some(true), Some(last)) => after = Some(last.to_owned()),
                    _ => return Ok(models),
                }
            }
        })
    }

    fn capabilities(&self, model: &str) -> BoxFuture<'_, Result<Capabilities, ProviderError>> {
        let model = model.to_owned();
        Box::pin(async move {
            let m = self.model_meta(&model).await?;
            Ok(Capabilities {
                // Every Claude model supports client tools.
                tool_calling: true,
                vision: m.vision,
                thinking: m.thinking,
                context_window: m.max_input_tokens,
                max_output_tokens: m.max_tokens,
            })
        })
    }

    fn stream(&self, request: ChatRequest) -> BoxFuture<'_, Result<EventStream, ProviderError>> {
        Box::pin(async move {
            let meta = self.model_meta(&request.model).await?;
            let built = request::build(&request, &meta, self.uses_claude_api());
            let body = serde_json::to_vec(&built.body)
                .map_err(|e| ProviderError::InvalidRequest(e.to_string()))?;
            let resp = self
                .transport
                .send(HttpRequest {
                    method: Method::Post,
                    url: self.url("/v1/messages"),
                    headers: self.headers(&built.betas),
                    body: Some(body),
                })
                .await?;
            let resp = ok_or_error(resp, Some(&self.config.api_key)).await?;
            Ok(decode(resp))
        })
    }
}

fn decode(resp: HttpResponse) -> EventStream {
    struct State {
        events:
            futures_util::stream::BoxStream<'static, Result<crate::sse::SseEvent, ProviderError>>,
        decoder: stream::Decoder,
        queue: VecDeque<StreamEvent>,
        finished: bool,
    }
    use crate::StreamEvent;
    let state = State {
        events: sse_events(resp.body),
        decoder: stream::Decoder::default(),
        queue: VecDeque::new(),
        finished: false,
    };
    fstream::unfold(state, |mut s| async move {
        loop {
            if let Some(ev) = s.queue.pop_front() {
                return Some((Ok(ev), s));
            }
            if s.finished || s.decoder.is_done() {
                return None;
            }
            match s.events.next().await {
                Some(Ok(sse)) => match s.decoder.push(&sse) {
                    Ok(evs) => s.queue.extend(evs),
                    Err(e) => {
                        s.finished = true;
                        return Some((Err(e), s));
                    }
                },
                Some(Err(e)) => {
                    s.finished = true;
                    return Some((Err(e), s));
                }
                None => {
                    // Body ended without `message_stop`: the connection dropped.
                    s.finished = true;
                    return Some((Err(ProviderError::Incomplete), s));
                }
            }
        }
    })
    .boxed()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn model_meta_from_models_api() {
        let v = json!({
            "id": "claude-opus-5-5",
            "max_input_tokens": 1_000_000,
            "max_tokens": 128_000,
            "capabilities": {
                "image_input": {"supported": true},
                "thinking": {"supported": true, "types": {"enabled": {"supported": false}, "adaptive": {"supported": true}}},
                "effort": {"supported": true, "low": {"supported": true}, "medium": {"supported": true},
                           "high": {"supported": true}, "xhigh": {"supported": true}, "max": {"supported": true}}
            }
        });
        let m = ModelMeta::from_json(&v);
        assert_eq!(m.max_input_tokens, Some(1_000_000));
        assert!(m.vision && m.adaptive_thinking && !m.budget_thinking);
        assert_eq!(m.effort_levels.len(), 5);
        assert_eq!(m.effort_at_most(Effort::XHigh), Some(Effort::XHigh));
    }

    #[test]
    fn effort_steps_down_or_disappears() {
        let m = ModelMeta {
            effort_levels: vec![Effort::Medium, Effort::High],
            ..ModelMeta::default()
        };
        assert_eq!(m.effort_at_most(Effort::Max), Some(Effort::High));
        assert_eq!(m.effort_at_most(Effort::Low), None);
    }

    #[test]
    fn config_debug_hides_key() {
        let shown = format!("{:?}", AnthropicConfig::new("sk-ant-secret"));
        assert!(!shown.contains("secret"));
    }
}
