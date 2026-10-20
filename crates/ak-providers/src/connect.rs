//! Setting up a connection (the connect wizard): the provider for a saved
//! [`AiConnection`], and the "test connection" steps with errors the user can act on.

use std::sync::Arc;

use ak_types::ipc::{ConnectError, ModelCheck, ModelChoice};
use ak_types::settings::AiConnection;

use crate::anthropic::{self, Anthropic, AnthropicConfig};
use crate::credentials::ApiKey;
use crate::openai::{Flavor, OpenAiCompat, OpenAiCompatConfig, WireApi};
use crate::{HttpTransport, Provider, ProviderError};

/// The provider `conn` names, with its key (cloud) or none (local servers).
pub fn provider_for(
    conn: &AiConnection,
    key: Option<ApiKey>,
    transport: Arc<dyn HttpTransport>,
) -> Result<Arc<dyn Provider>, ConnectError> {
    let base = conn
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|u| !u.is_empty());
    if conn.provider == anthropic::ID {
        let mut config = AnthropicConfig::new(key.ok_or(ConnectError::WrongKey)?);
        if let Some(url) = base {
            config.base_url = url.trim_end_matches('/').to_owned();
        }
        return Ok(Arc::new(Anthropic::new(config, transport)));
    }
    let flavor = match conn.provider.as_str() {
        "openai" => Flavor::OpenAi,
        "ollama" => Flavor::Ollama,
        "lmstudio" => Flavor::LmStudio,
        "openrouter" => Flavor::OpenRouter,
        "openai-compatible" => Flavor::Generic,
        other => {
            return Err(ConnectError::Other {
                detail: format!("unknown provider `{other}`"),
            });
        }
    };
    if matches!(flavor, Flavor::OpenAi | Flavor::OpenRouter) && key.is_none() {
        return Err(ConnectError::WrongKey);
    }
    let base_url = base
        .map(str::to_owned)
        .or_else(|| flavor.default_base_url().map(str::to_owned))
        .ok_or_else(|| ConnectError::WrongAddress {
            detail: "no address given".into(),
        })?;
    let config = OpenAiCompatConfig {
        base_url: base_url.trim_end_matches('/').to_owned(),
        api_key: key,
        flavor,
        api: WireApi::default_for(flavor),
    };
    Ok(Arc::new(OpenAiCompat::new(config, transport)))
}

/// The models the service offers ("Test connection").
pub async fn list_models(provider: &dyn Provider) -> Result<Vec<ModelChoice>, ConnectError> {
    let models = provider.list_models().await.map_err(listing_error)?;
    if models.is_empty() {
        return Err(ConnectError::NoModels);
    }
    Ok(models
        .into_iter()
        .map(|m| ModelChoice {
            id: m.id,
            display_name: m.display_name,
        })
        .collect())
}

/// What the chosen model can do.
pub async fn check_model(provider: &dyn Provider, model: &str) -> Result<ModelCheck, ConnectError> {
    let caps = provider.capabilities(model).await.map_err(|e| match e {
        ProviderError::ModelNotFound(detail) => ConnectError::Other { detail },
        other => listing_error(other),
    })?;
    Ok(ModelCheck {
        tool_calling: caps.tool_calling,
        vision: caps.vision,
    })
}

fn listing_error(e: ProviderError) -> ConnectError {
    match e {
        ProviderError::Network(detail) | ProviderError::Transport(detail) => {
            ConnectError::Unreachable { detail }
        }
        ProviderError::Auth => ConnectError::WrongKey,
        ProviderError::RateLimited { .. } => ConnectError::RateLimited,
        // 404 on the model list, or a web page instead of JSON.
        ProviderError::ModelNotFound(detail) | ProviderError::Decode(detail) => {
            ConnectError::WrongAddress { detail }
        }
        other => ConnectError::Other {
            detail: other.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use futures_util::future::BoxFuture;
    use futures_util::stream::{self, StreamExt};

    use super::*;
    use crate::{HttpRequest, HttpResponse};

    /// Answers every request with one status and body; remembers the URLs.
    struct Canned {
        status: u16,
        body: &'static str,
        urls: std::sync::Mutex<Vec<String>>,
    }

    impl HttpTransport for Canned {
        fn send(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, ProviderError>> {
            if let Ok(mut u) = self.urls.lock() {
                u.push(request.url);
            }
            let (status, body) = (self.status, self.body);
            Box::pin(async move {
                if status == 0 {
                    return Err(ProviderError::Network("connection refused".into()));
                }
                Ok(HttpResponse {
                    status,
                    headers: vec![("content-type".into(), "application/json".into())],
                    body: stream::iter([Ok(body.as_bytes().to_vec())]).boxed(),
                })
            })
        }
    }

    fn canned(status: u16, body: &'static str) -> Arc<Canned> {
        Arc::new(Canned {
            status,
            body,
            urls: std::sync::Mutex::new(Vec::new()),
        })
    }

    fn conn(provider: &str, base_url: Option<&str>) -> AiConnection {
        AiConnection {
            provider: provider.into(),
            base_url: base_url.map(Into::into),
            model: String::new(),
            effort: None,
        }
    }

    async fn models(
        provider: &str,
        base_url: Option<&str>,
        key: Option<&str>,
        t: Arc<Canned>,
    ) -> Result<Vec<ModelChoice>, ConnectError> {
        let p = provider_for(&conn(provider, base_url), key.and_then(ApiKey::new), t)?;
        list_models(p.as_ref()).await
    }

    #[tokio::test]
    async fn lists_models_from_the_right_address() {
        let t = canned(200, r#"{"data":[{"id":"qwen3:8b"},{"id":"llama4"}]}"#);
        let list = models("ollama", None, None, t.clone()).await;
        assert_eq!(
            list.map(|l| l.into_iter().map(|m| m.id).collect::<Vec<_>>()),
            Ok(vec!["qwen3:8b".to_owned(), "llama4".to_owned()])
        );
        assert_eq!(
            t.urls.lock().map(|u| u.clone()).unwrap_or_default(),
            ["http://localhost:11434/v1/models"]
        );

        let t = canned(200, r#"{"data":[{"id":"m"}]}"#);
        models(
            "openai-compatible",
            Some("http://10.0.0.5:8080/v1/ "),
            None,
            t.clone(),
        )
        .await
        .ok();
        assert_eq!(
            t.urls.lock().map(|u| u.clone()).unwrap_or_default(),
            ["http://10.0.0.5:8080/v1/models"]
        );
    }

    #[tokio::test]
    async fn errors_say_what_to_fix() {
        let key = Some("sk-test");
        assert!(matches!(
            models("ollama", None, None, canned(0, "")).await,
            Err(ConnectError::Unreachable { .. })
        ));
        assert_eq!(
            models(
                "openai",
                None,
                key,
                canned(401, r#"{"error":{"message":"bad key"}}"#)
            )
            .await,
            Err(ConnectError::WrongKey)
        );
        assert!(matches!(
            models("lmstudio", None, None, canned(404, "Not Found")).await,
            Err(ConnectError::WrongAddress { .. })
        ));
        assert!(matches!(
            models(
                "openai-compatible",
                Some("http://x/"),
                None,
                canned(200, "<html>hi</html>")
            )
            .await,
            Err(ConnectError::WrongAddress { .. })
        ));
        assert_eq!(
            models("ollama", None, None, canned(200, r#"{"data":[]}"#)).await,
            Err(ConnectError::NoModels)
        );
        // Cloud services need a key; a custom service needs an address.
        assert_eq!(
            models("openrouter", None, None, canned(200, "{}")).await,
            Err(ConnectError::WrongKey)
        );
        assert_eq!(
            models("anthropic", None, Some("  "), canned(200, "{}")).await,
            Err(ConnectError::WrongKey)
        );
        assert!(matches!(
            models("openai-compatible", Some(" "), None, canned(200, "{}")).await,
            Err(ConnectError::WrongAddress { .. })
        ));
    }
}
