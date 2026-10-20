//! Calls to AgentKnife's app (ak-app) over Tauri IPC (`ak_types::ipc`). In a
//! plain browser (`trunk serve`) there is no app: a demo answers instead, so
//! every screen can be tried.

use std::sync::{Arc, Mutex};

use ak_types::ipc::{
    ConnectError, ConnectRequest, HomeState, ModelCheck, ModelChoice, NewProject, ProjectRow, cmd,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallError {
    /// A connection problem the wizard explains.
    Connect(ConnectError),
    /// Anything else (shown under "Details").
    Other(String),
}

impl From<ConnectError> for CallError {
    fn from(e: ConnectError) -> Self {
        CallError::Connect(e)
    }
}

#[derive(Clone)]
pub enum Backend {
    Tauri,
    Demo(Arc<Mutex<Demo>>),
}

impl Backend {
    /// Tauri when running inside AgentKnife, the demo otherwise.
    pub fn detect() -> Backend {
        #[cfg(target_arch = "wasm32")]
        if tauri::available() {
            return Backend::Tauri;
        }
        Backend::Demo(Arc::new(Mutex::new(Demo::new())))
    }

    pub fn is_demo(&self) -> bool {
        matches!(self, Backend::Demo(_))
    }

    pub async fn home_state(&self) -> Result<HomeState, CallError> {
        self.call(cmd::HOME_STATE, None::<()>).await
    }

    pub async fn create_project(&self, name: &str) -> Result<ProjectRow, CallError> {
        let request = NewProject {
            name: name.to_owned(),
        };
        self.call(cmd::CREATE_PROJECT, Some(request)).await
    }

    pub async fn open_project(&self, id: &str) -> Result<ProjectRow, CallError> {
        self.call(cmd::OPEN_PROJECT, Some(id)).await
    }

    pub async fn open_folder(&self) -> Result<Option<ProjectRow>, CallError> {
        self.call(cmd::OPEN_FOLDER, None::<()>).await
    }

    pub async fn list_models(&self, r: &ConnectRequest) -> Result<Vec<ModelChoice>, CallError> {
        self.call(cmd::LIST_MODELS, Some(r)).await
    }

    pub async fn check_model(&self, r: &ConnectRequest) -> Result<ModelCheck, CallError> {
        self.call(cmd::CHECK_MODEL, Some(r)).await
    }

    pub async fn save_connection(&self, r: &ConnectRequest) -> Result<(), CallError> {
        self.call(cmd::SAVE_CONNECTION, Some(r)).await
    }

    async fn call<R: DeserializeOwned>(
        &self,
        command: &str,
        request: Option<impl Serialize>,
    ) -> Result<R, CallError> {
        let request = match request {
            Some(r) => Some(serde_json::to_value(r).map_err(|e| CallError::Other(e.to_string()))?),
            None => None,
        };
        let answer = match self {
            #[cfg(target_arch = "wasm32")]
            Backend::Tauri => tauri::invoke(command, request).await,
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Tauri => Err(Value::String("Tauri runs only in the window".into())),
            Backend::Demo(demo) => demo
                .lock()
                .map_err(|_| Value::String("demo state poisoned".into()))
                .and_then(|mut d| d.answer(command, request)),
        };
        match answer {
            Ok(v) => serde_json::from_value(v).map_err(|e| CallError::Other(e.to_string())),
            Err(e) => Err(serde_json::from_value::<ConnectError>(e.clone())
                .map(CallError::Connect)
                .unwrap_or_else(|_| {
                    CallError::Other(e.as_str().map_or_else(|| e.to_string(), str::to_owned))
                })),
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod tauri {
    //! `window.__TAURI_INTERNALS__.invoke(cmd, args)`, reached through
    //! `Reflect` so no extern block (and no unsafe) is needed.

    use js_sys::{Function, JSON, Promise, Reflect};
    use serde_json::Value;
    use wasm_bindgen::{JsCast, JsValue};
    use wasm_bindgen_futures::JsFuture;

    fn internals() -> Option<JsValue> {
        let window = web_sys::window()?;
        Reflect::get(&window, &JsValue::from_str("__TAURI_INTERNALS__"))
            .ok()
            .filter(|v| v.is_object())
    }

    pub fn available() -> bool {
        internals().is_some()
    }

    fn to_js(v: &Value) -> JsValue {
        JSON::parse(&v.to_string()).unwrap_or(JsValue::NULL)
    }

    fn from_js(v: &JsValue) -> Value {
        if v.is_undefined() {
            return Value::Null;
        }
        JSON::stringify(v)
            .ok()
            .and_then(|s| s.as_string())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| Value::String(format!("{v:?}")))
    }

    pub async fn invoke(command: &str, request: Option<Value>) -> Result<Value, Value> {
        let missing = || Value::String("not running inside AgentKnife".into());
        let internals = internals().ok_or_else(missing)?;
        let invoke = Reflect::get(&internals, &JsValue::from_str("invoke"))
            .ok()
            .and_then(|f| f.dyn_into::<Function>().ok())
            .ok_or_else(missing)?;
        let args = match request {
            Some(r) => to_js(&serde_json::json!({ "request": r })),
            None => to_js(&serde_json::json!({})),
        };
        let promise = invoke
            .call2(&internals, &JsValue::from_str(command), &args)
            .map_err(|e| from_js(&e))?
            .dyn_into::<Promise>()
            .map_err(|e| from_js(&e))?;
        JsFuture::from(promise)
            .await
            .map(|v| from_js(&v))
            .map_err(|e| from_js(&e))
    }
}

/// In-memory stand-in for the app, for trying the screens in a browser. Keys
/// containing `bad` are refused; addresses containing `nothing` do not answer;
/// models with `3b` in the name cannot use tools.
pub struct Demo {
    projects: Vec<ProjectRow>,
    ai: Option<ak_types::settings::AiConnection>,
    next: u32,
}

const DEMO_FOLDER: &str = "~/AgentKnife/Projects";

impl Demo {
    pub fn new() -> Demo {
        let now = now();
        Demo {
            projects: vec![
                ProjectRow {
                    id: "demo-1".into(),
                    name: "Todo List".into(),
                    path: format!("{DEMO_FOLDER}/Todo List"),
                    last_opened_at: now.saturating_sub(3_600),
                    exists: true,
                },
                ProjectRow {
                    id: "demo-2".into(),
                    name: "Recipe Box".into(),
                    path: "~/Desktop/Recipe Box".into(),
                    last_opened_at: now.saturating_sub(9 * 86_400),
                    exists: false,
                },
            ],
            ai: None,
            next: 3,
        }
    }

    fn answer(&mut self, command: &str, request: Option<Value>) -> Result<Value, Value> {
        let req = request.unwrap_or(Value::Null);
        let err = |e: ConnectError| serde_json::to_value(e).unwrap_or(Value::Null);
        let ok = |v: Value| Ok(v);
        match command {
            cmd::HOME_STATE => {
                let mut projects = self.projects.clone();
                projects.sort_by_key(|p| std::cmp::Reverse(p.last_opened_at));
                ok(json!(HomeState {
                    projects,
                    ai: self.ai.clone(),
                    projects_folder: DEMO_FOLDER.into(),
                }))
            }
            cmd::CREATE_PROJECT => {
                let name = req["name"].as_str().unwrap_or_default().trim().to_owned();
                let row = ProjectRow {
                    id: format!("demo-{}", self.next),
                    path: format!("{DEMO_FOLDER}/{name}"),
                    name,
                    last_opened_at: now(),
                    exists: true,
                };
                self.next += 1;
                self.projects.push(row.clone());
                ok(json!(row))
            }
            cmd::OPEN_PROJECT => {
                let id = req.as_str().unwrap_or_default();
                let row = self
                    .projects
                    .iter_mut()
                    .find(|p| p.id == id)
                    .ok_or_else(|| Value::String(format!("no project {id}")))?;
                row.last_opened_at = now();
                ok(json!(row))
            }
            cmd::OPEN_FOLDER => ok(Value::Null),
            cmd::LIST_MODELS | cmd::CHECK_MODEL => {
                let r: ConnectRequest =
                    serde_json::from_value(req).map_err(|e| Value::String(e.to_string()))?;
                let key = r.api_key.as_deref().unwrap_or_default().trim();
                let address = r.connection.base_url.clone().unwrap_or_default();
                let needs_key =
                    ak_types::ipc::preset(&r.connection.provider).is_some_and(|p| p.needs_key);
                if (needs_key && key.is_empty()) || key.contains("bad") {
                    return Err(err(ConnectError::WrongKey));
                }
                if address.contains("nothing") {
                    return Err(err(ConnectError::Unreachable {
                        detail: format!("error sending request for url ({address}/models)"),
                    }));
                }
                if command == cmd::CHECK_MODEL {
                    let m = &r.connection.model;
                    return ok(json!(ModelCheck {
                        tool_calling: !m.contains("3b"),
                        vision: m.starts_with("claude") || m.starts_with("gpt"),
                    }));
                }
                let ids: &[&str] = match r.connection.provider.as_str() {
                    "anthropic" => &["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"],
                    "openai" => &["gpt-5.2", "gpt-5-mini"],
                    _ => &["qwen3:8b", "llama3.2:3b"],
                };
                ok(json!(
                    ids.iter()
                        .map(|id| ModelChoice {
                            id: (*id).to_owned(),
                            display_name: None,
                        })
                        .collect::<Vec<_>>()
                ))
            }
            cmd::SAVE_CONNECTION => {
                let r: ConnectRequest =
                    serde_json::from_value(req).map_err(|e| Value::String(e.to_string()))?;
                self.ai = Some(r.connection);
                ok(Value::Null)
            }
            other => Err(Value::String(format!("unknown command {other}"))),
        }
    }
}

impl Default for Demo {
    fn default() -> Self {
        Demo::new()
    }
}

/// Unix seconds.
pub fn now() -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        (js_sys::Date::now() / 1000.0) as u64
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use ak_types::settings::AiConnection;

    use super::*;

    fn block<F: std::future::Future>(f: F) -> F::Output {
        // The demo never awaits anything: one poll finishes it.
        let mut f = std::pin::pin!(f);
        let waker = std::task::Waker::noop();
        match f.as_mut().poll(&mut std::task::Context::from_waker(waker)) {
            std::task::Poll::Ready(v) => v,
            std::task::Poll::Pending => panic!("demo call did not finish"),
        }
    }

    fn request(provider: &str, key: Option<&str>, model: &str) -> ConnectRequest {
        ConnectRequest {
            connection: AiConnection {
                provider: provider.into(),
                base_url: None,
                model: model.into(),
                effort: None,
            },
            api_key: key.map(Into::into),
        }
    }

    #[test]
    fn the_demo_answers_like_the_app() {
        let b = Backend::Demo(Arc::new(Mutex::new(Demo::new())));
        let home = block(b.home_state()).unwrap();
        assert_eq!(home.projects.len(), 2);
        assert!(home.ai.is_none());

        let row = block(b.create_project("Shop")).unwrap();
        assert_eq!(block(b.home_state()).unwrap().projects[0].id, row.id);
        assert_eq!(block(b.open_project(&row.id)).unwrap().name, "Shop");

        assert_eq!(
            block(b.list_models(&request("anthropic", None, ""))),
            Err(CallError::Connect(ConnectError::WrongKey))
        );
        let models = block(b.list_models(&request("anthropic", Some("sk-1"), ""))).unwrap();
        assert_eq!(models[0].id, "claude-opus-5-5");
        let weak = block(b.check_model(&request("ollama", None, "llama3.2:3b"))).unwrap();
        assert!(!weak.tool_calling);

        block(b.save_connection(&request("ollama", None, "qwen3:8b"))).unwrap();
        assert_eq!(
            block(b.home_state()).unwrap().ai.map(|a| a.model),
            Some("qwen3:8b".into())
        );
        assert!(matches!(
            block(b.open_project("nope")),
            Err(CallError::Other(_))
        ));
    }
}
