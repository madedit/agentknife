//! OpenAI Chat Completions and compatible endpoints (Ollama, LM Studio, OpenRouter,
//! any other server speaking the same protocol).
//!
//! One adapter; a [`Flavor`] (detected from the base URL or set explicitly) selects
//! the few protocol differences and where model capabilities come from:
//! Ollama `POST /api/show`, LM Studio `GET /api/v1/models`, OpenRouter `GET /models`.

mod request;
mod stream;
mod think_tags;

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use futures_util::future::BoxFuture;
use futures_util::stream as fstream;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::credentials::ApiKey;
use crate::sse::sse_events;
use crate::transport::ok_or_error;
use crate::{
    Capabilities, ChatRequest, EventStream, HttpRequest, HttpResponse, HttpTransport, Method,
    ModelInfo, Provider, ProviderError, StreamEvent,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Flavor {
    OpenAi,
    Ollama,
    LmStudio,
    OpenRouter,
    /// Any other Chat Completions server (DeepSeek, Groq, vLLM, …).
    Generic,
}

impl Flavor {
    /// Provider id ([`ak_types::chat::ProviderReplay::origin`]).
    pub fn id(self) -> &'static str {
        match self {
            Flavor::OpenAi => "openai",
            Flavor::Ollama => "ollama",
            Flavor::LmStudio => "lmstudio",
            Flavor::OpenRouter => "openrouter",
            Flavor::Generic => "openai-compatible",
        }
    }

    pub fn default_base_url(self) -> Option<&'static str> {
        match self {
            Flavor::OpenAi => Some("https://api.openai.com/v1"),
            Flavor::Ollama => Some("http://localhost:11434/v1"),
            Flavor::LmStudio => Some("http://localhost:1234/v1"),
            Flavor::OpenRouter => Some("https://openrouter.ai/api/v1"),
            Flavor::Generic => None,
        }
    }

    /// Best guess from the base URL; the connect wizard lets the user correct it.
    pub fn detect(base_url: &str) -> Flavor {
        let url = base_url.to_ascii_lowercase();
        if url.contains("api.openai.com") {
            Flavor::OpenAi
        } else if url.contains("openrouter.ai") {
            Flavor::OpenRouter
        } else if url.contains(":11434") || url.contains("ollama.com") {
            Flavor::Ollama
        } else if url.contains(":1234") {
            Flavor::LmStudio
        } else {
            Flavor::Generic
        }
    }
}

#[derive(Clone)]
pub struct OpenAiCompatConfig {
    /// Including the `/v1` (or `/api/v1`) prefix.
    pub base_url: String,
    /// Not needed by local servers.
    pub api_key: Option<ApiKey>,
    pub flavor: Flavor,
}

impl OpenAiCompatConfig {
    /// Flavor detected from `base_url`.
    pub fn new(base_url: impl Into<String>, api_key: Option<ApiKey>) -> Self {
        let base_url = base_url.into();
        let flavor = Flavor::detect(&base_url);
        Self {
            base_url,
            api_key,
            flavor,
        }
    }

    /// Default base URL for a known flavor.
    pub fn for_flavor(flavor: Flavor, api_key: Option<ApiKey>) -> Self {
        Self {
            base_url: flavor.default_base_url().unwrap_or_default().to_owned(),
            api_key,
            flavor,
        }
    }
}

impl fmt::Debug for OpenAiCompatConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAiCompatConfig")
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("flavor", &self.flavor)
            .finish()
    }
}

pub struct OpenAiCompat {
    config: OpenAiCompatConfig,
    transport: Arc<dyn HttpTransport>,
    caps: Mutex<HashMap<String, Capabilities>>,
    /// LM Studio / OpenRouter capability lists, fetched once (OpenRouter's is large).
    model_list: Mutex<Option<Value>>,
}

impl OpenAiCompat {
    pub fn new(config: OpenAiCompatConfig, transport: Arc<dyn HttpTransport>) -> Self {
        Self {
            config,
            transport,
            caps: Mutex::new(HashMap::new()),
            model_list: Mutex::new(None),
        }
    }

    async fn cached_list(&self, url: String) -> Result<Value, ProviderError> {
        if let Some(list) = self.model_list.lock().ok().and_then(|l| l.clone()) {
            return Ok(list);
        }
        let list = self.call_json(Method::Get, url, None).await?;
        if let Ok(mut slot) = self.model_list.lock() {
            *slot = Some(list.clone());
        }
        Ok(list)
    }

    pub fn flavor(&self) -> Flavor {
        self.config.flavor
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.config.base_url.trim_end_matches('/'))
    }

    /// Server root without the OpenAI-compatible prefix, for native endpoints.
    fn root_url(&self, path: &str) -> String {
        let base = self.config.base_url.trim_end_matches('/');
        let root = base
            .strip_suffix("/api/v1")
            .or_else(|| base.strip_suffix("/v1"))
            .unwrap_or(base);
        format!("{root}{path}")
    }

    fn headers(&self) -> Vec<(String, String)> {
        let mut h = vec![("content-type".to_owned(), "application/json".to_owned())];
        if let Some(key) = self
            .config
            .api_key
            .as_ref()
            .filter(|k| !k.expose().is_empty())
        {
            h.push((
                "authorization".to_owned(),
                format!("Bearer {}", key.expose()),
            ));
        }
        if self.config.flavor == Flavor::OpenRouter {
            h.push(("x-openrouter-title".to_owned(), "AgentKnife".to_owned()));
        }
        h
    }

    async fn call_json(
        &self,
        method: Method,
        url: String,
        body: Option<&Value>,
    ) -> Result<Value, ProviderError> {
        let resp = self
            .transport
            .send(HttpRequest {
                method,
                url,
                headers: self.headers(),
                body: body.map(Value::to_string).map(String::into_bytes),
            })
            .await?;
        let text = ok_or_error(resp, self.config.api_key.as_ref())
            .await?
            .text()
            .await?;
        serde_json::from_str(&text).map_err(|e| ProviderError::Decode(e.to_string()))
    }

    async fn probe(&self, model: &str) -> Result<Capabilities, ProviderError> {
        match self.config.flavor {
            Flavor::Ollama => {
                let show = self
                    .call_json(
                        Method::Post,
                        self.root_url("/api/show"),
                        Some(&json!({ "model": model })),
                    )
                    .await?;
                Ok(ollama_caps(&show))
            }
            Flavor::LmStudio => {
                let list = self.cached_list(self.root_url("/api/v1/models")).await?;
                lmstudio_caps(&list, model)
                    .ok_or_else(|| ProviderError::ModelNotFound(model.to_owned()))
            }
            Flavor::OpenRouter => {
                let list = self.cached_list(self.url("/models")).await?;
                openrouter_caps(&list, model)
                    .ok_or_else(|| ProviderError::ModelNotFound(model.to_owned()))
            }
            // Some servers (llama.cpp-style) put context size and input modalities in
            // their /models entries; tool support is never listed, so assume it.
            Flavor::Generic => {
                let list = self.cached_list(self.url("/models")).await.ok();
                Ok(list
                    .and_then(|l| generic_caps(&l, model))
                    .unwrap_or(Capabilities {
                        tool_calling: true,
                        ..Capabilities::default()
                    }))
            }
            // OpenAI's /models has no capability data (Responses adapter: P1-PROV-005).
            Flavor::OpenAi => Ok(Capabilities {
                tool_calling: true,
                ..Capabilities::default()
            }),
        }
    }
}

/// Best effort for unknown servers: `meta.n_ctx` (llama.cpp), `context_length`, or
/// `max_context_length`, plus `architecture.input_modalities`.
fn generic_caps(list: &Value, model: &str) -> Option<Capabilities> {
    let m = list["data"].as_array()?.iter().find(|m| m["id"] == model)?;
    let context = [
        &m["meta"]["n_ctx"],
        &m["context_length"],
        &m["max_context_length"],
    ]
    .into_iter()
    .find_map(|v| v.as_u64().and_then(|n| u32::try_from(n).ok()));
    Some(Capabilities {
        tool_calling: true,
        vision: m["architecture"]["input_modalities"]
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v == "image")),
        thinking: false,
        context_window: context,
        max_output_tokens: None,
    })
}

fn ollama_caps(show: &Value) -> Capabilities {
    let has = |c: &str| {
        show["capabilities"]
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v == c))
    };
    let arch = show["model_info"]["general.architecture"]
        .as_str()
        .unwrap_or_default();
    let context = show["model_info"][format!("{arch}.context_length")]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok());
    Capabilities {
        tool_calling: has("tools"),
        vision: has("vision"),
        thinking: has("thinking"),
        context_window: context,
        max_output_tokens: None,
    }
}

/// LM Studio's list shape varies between versions: accept `data`, `models`, or a
/// bare array, and `key` or `id`.
fn lmstudio_caps(list: &Value, model: &str) -> Option<Capabilities> {
    let items = list["data"]
        .as_array()
        .or(list["models"].as_array())
        .or(list.as_array())?;
    let m = items
        .iter()
        .find(|m| m["key"] == model || m["id"] == model)?;
    let caps = &m["capabilities"];
    let flag = |v: &Value| v.as_bool().unwrap_or(false);
    Some(Capabilities {
        tool_calling: flag(&caps["trained_for_tool_use"]),
        vision: flag(&caps["vision"]),
        thinking: !caps["reasoning"].is_null() || !m["reasoning"].is_null(),
        context_window: m["max_context_length"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok()),
        max_output_tokens: None,
    })
}

fn openrouter_caps(list: &Value, model: &str) -> Option<Capabilities> {
    let m = list["data"].as_array()?.iter().find(|m| m["id"] == model)?;
    let params = |p: &str| {
        m["supported_parameters"]
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v == p))
    };
    let num = |v: &Value| v.as_u64().and_then(|n| u32::try_from(n).ok());
    Some(Capabilities {
        tool_calling: params("tools"),
        vision: m["architecture"]["input_modalities"]
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v == "image")),
        thinking: params("reasoning"),
        context_window: num(&m["context_length"]),
        max_output_tokens: num(&m["top_provider"]["max_completion_tokens"]),
    })
}

impl Provider for OpenAiCompat {
    fn id(&self) -> &'static str {
        self.config.flavor.id()
    }

    fn list_models(&self) -> BoxFuture<'_, Result<Vec<ModelInfo>, ProviderError>> {
        Box::pin(async move {
            let list = self
                .call_json(Method::Get, self.url("/models"), None)
                .await?;
            Ok(list["data"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|m| {
                    Some(ModelInfo {
                        id: m["id"].as_str()?.to_owned(),
                        display_name: m["name"].as_str().map(str::to_owned),
                    })
                })
                .collect())
        })
    }

    fn capabilities(&self, model: &str) -> BoxFuture<'_, Result<Capabilities, ProviderError>> {
        let model = model.to_owned();
        Box::pin(async move {
            if let Some(c) = self.caps.lock().ok().and_then(|c| c.get(&model).copied()) {
                return Ok(c);
            }
            let caps = self.probe(&model).await?;
            if let Ok(mut cache) = self.caps.lock() {
                cache.insert(model, caps);
            }
            Ok(caps)
        })
    }

    fn stream(&self, request: ChatRequest) -> BoxFuture<'_, Result<EventStream, ProviderError>> {
        Box::pin(async move {
            let body = request::build(&request, self.config.flavor);
            let resp = self
                .transport
                .send(HttpRequest {
                    method: Method::Post,
                    url: self.url("/chat/completions"),
                    headers: self.headers(),
                    body: Some(body.to_string().into_bytes()),
                })
                .await?;
            let resp = ok_or_error(resp, self.config.api_key.as_ref()).await?;
            Ok(decode(resp, self.config.flavor))
        })
    }
}

fn decode(resp: HttpResponse, flavor: Flavor) -> EventStream {
    struct State {
        events:
            futures_util::stream::BoxStream<'static, Result<crate::sse::SseEvent, ProviderError>>,
        decoder: stream::Decoder,
        queue: VecDeque<StreamEvent>,
        finished: bool,
    }
    let state = State {
        events: sse_events(resp.body),
        decoder: stream::Decoder::new(flavor),
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
            let decoded = match s.events.next().await {
                Some(Ok(sse)) => s.decoder.push(&sse),
                Some(Err(e)) => Err(e),
                None => {
                    s.finished = true;
                    s.decoder.end_of_stream()
                }
            };
            match decoded {
                Ok(evs) => s.queue.extend(evs),
                Err(e) => {
                    s.finished = true;
                    return Some((Err(e), s));
                }
            }
        }
    })
    .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flavor_detection() {
        assert_eq!(Flavor::detect("https://api.openai.com/v1"), Flavor::OpenAi);
        assert_eq!(Flavor::detect("http://localhost:11434/v1"), Flavor::Ollama);
        assert_eq!(Flavor::detect("http://127.0.0.1:1234/v1"), Flavor::LmStudio);
        assert_eq!(
            Flavor::detect("https://openrouter.ai/api/v1"),
            Flavor::OpenRouter
        );
        assert_eq!(
            Flavor::detect("https://api.deepseek.com/v1"),
            Flavor::Generic
        );
    }

    #[test]
    fn root_url_strips_compat_prefix() {
        let p = |base: &str| {
            OpenAiCompat::new(
                OpenAiCompatConfig::new(base, None),
                Arc::new(crate::fixture::FixtureTransport::new(vec![])),
            )
        };
        assert_eq!(
            p("http://localhost:11434/v1").root_url("/api/show"),
            "http://localhost:11434/api/show"
        );
        assert_eq!(
            p("http://localhost:1234/api/v1/").root_url("/x"),
            "http://localhost:1234/x"
        );
    }

    #[test]
    fn ollama_capabilities() {
        let show = json!({
            "capabilities": ["completion", "vision"],
            "model_info": { "general.architecture": "gemma2", "gemma2.context_length": 8192 }
        });
        let c = ollama_caps(&show);
        assert!(!c.tool_calling, "no `tools` capability => flagged");
        assert!(c.vision);
        assert_eq!(c.context_window, Some(8192));
    }

    #[test]
    fn generic_capabilities_from_llama_cpp_style_list() {
        let list = json!({ "data": [{
            "id": "qwen3.8-flash-next-gptq-nvfp4",
            "meta": { "n_ctx": 245760 },
            "architecture": { "input_modalities": ["text", "image"] }
        }]});
        let c = generic_caps(&list, "qwen3.8-flash-next-gptq-nvfp4").unwrap();
        assert!(c.tool_calling && c.vision);
        assert_eq!(c.context_window, Some(245_760));
        assert!(generic_caps(&list, "other").is_none());
    }

    #[test]
    fn config_debug_hides_key() {
        let shown = format!(
            "{:?}",
            OpenAiCompatConfig::new("https://openrouter.ai/api/v1", Some("sk-or-secret".into()))
        );
        assert!(!shown.contains("secret"));
    }
}
