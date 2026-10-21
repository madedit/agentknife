//! Calls to AgentKnife's app (ak-app) over Tauri IPC (`ak_types::ipc`). In a
//! plain browser (`trunk serve`) there is no app: a demo answers instead, so
//! every screen can be tried.

use std::sync::{Arc, Mutex};

use ak_types::agent::{AgentEvent, PlanDecision};
use ak_types::doctor::SystemAction;
use ak_types::ipc::CrashReports;
use ak_types::ipc::{
    AGENT_EVENT, Answer, BundleFailure, BundleResult, ConnectError, ConnectRequest, DeleteProject,
    DoctorCheck, FileEntry, FileText, HomeState, InterruptionChoice, LOG_EVENT, MarkVersion,
    ModelCheck, ModelChoice, NewProject, OpenedProject, PREVIEW_EVENT, SaveFile, SavedConnection,
    SendMessage, StuckChoice, TimelineRow, cmd,
};
use ak_types::log::LogLine;
use ak_types::preview::PreviewStatus;
use ak_types::settings::AppSettings;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::demo::Demo;

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

/// A demo listener: the raw payload of one event.
pub type Handler = Arc<dyn Fn(serde_json::Value) + Send + Sync>;

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
        Backend::demo()
    }

    pub fn demo() -> Backend {
        Backend::Demo(Arc::new(Mutex::new(Demo::new())))
    }

    pub fn is_demo(&self) -> bool {
        matches!(self, Backend::Demo(_))
    }

    /// Calls `f` with every `event` (a Tauri event name from `ak_types::ipc`)
    /// from now on; payloads that do not parse as `T` are skipped.
    pub fn on<T: DeserializeOwned + 'static>(
        &self,
        event: &'static str,
        f: impl Fn(T) + Send + Sync + 'static,
    ) {
        let handler = move |payload: serde_json::Value| {
            if let Ok(v) = serde_json::from_value::<T>(payload) {
                f(v);
            }
        };
        match self {
            #[cfg(target_arch = "wasm32")]
            Backend::Tauri => tauri::listen(event, handler),
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Tauri => {
                let _ = (event, handler);
            }
            Backend::Demo(demo) => {
                if let Ok(mut d) = demo.lock() {
                    d.listeners
                        .entry(event)
                        .or_default()
                        .push(Arc::new(handler));
                }
            }
        }
    }

    /// The agent's events for the open project.
    pub fn listen(&self, f: impl Fn(AgentEvent) + Send + Sync + 'static) {
        self.on(AGENT_EVENT, f);
    }

    /// The open project's preview status, whenever it changes.
    pub fn listen_preview(&self, f: impl Fn(PreviewStatus) + Send + Sync + 'static) {
        self.on(PREVIEW_EVENT, f);
    }

    pub async fn preview_status(&self) -> Result<PreviewStatus, CallError> {
        self.call(cmd::PREVIEW_STATUS, None::<()>).await
    }

    pub async fn run_app(&self) -> Result<(), CallError> {
        self.call(cmd::RUN_APP, None::<()>).await
    }

    /// New lines of the open project's log, a batch at a time.
    pub fn listen_log(&self, f: impl Fn(Vec<LogLine>) + Send + Sync + 'static) {
        self.on(LOG_EVENT, f);
    }

    pub async fn get_log(&self) -> Result<Vec<LogLine>, CallError> {
        self.call(cmd::GET_LOG, None::<()>).await
    }

    pub async fn clear_log(&self) -> Result<(), CallError> {
        self.call(cmd::CLEAR_LOG, None::<()>).await
    }

    pub async fn list_files(&self) -> Result<Vec<FileEntry>, CallError> {
        self.call(cmd::LIST_FILES, None::<()>).await
    }

    pub async fn read_file(&self, path: &str) -> Result<FileText, CallError> {
        self.call(cmd::READ_FILE, Some(path)).await
    }

    pub async fn write_file(&self, path: &str, text: &str) -> Result<(), CallError> {
        let request = SaveFile {
            path: path.to_owned(),
            text: text.to_owned(),
        };
        self.call(cmd::WRITE_FILE, Some(request)).await
    }

    pub async fn timeline(&self) -> Result<Vec<TimelineRow>, CallError> {
        self.call(cmd::TIMELINE, None::<()>).await
    }

    pub async fn restore(&self, id: &str) -> Result<(), CallError> {
        self.call(cmd::RESTORE, Some(id)).await
    }

    pub async fn doctor_check(&self) -> Result<DoctorCheck, CallError> {
        self.call(cmd::DOCTOR_CHECK, None::<()>).await
    }

    pub async fn doctor_install(&self) -> Result<DoctorCheck, CallError> {
        self.call(cmd::DOCTOR_INSTALL, None::<()>).await
    }

    pub async fn doctor_action(&self, action: &SystemAction) -> Result<(), CallError> {
        self.call(cmd::DOCTOR_ACTION, Some(action)).await
    }

    /// Packages the app; a failure comes back as its [`BundleFailure`].
    pub async fn bundle(&self) -> Result<BundleResult, Result<BundleFailure, CallError>> {
        match self.call_raw(cmd::BUNDLE, None::<()>).await {
            Ok(v) => serde_json::from_value(v).map_err(|e| Err(CallError::Other(e.to_string()))),
            Err(e) => Err(
                serde_json::from_value::<BundleFailure>(e.clone()).map_err(|_| {
                    CallError::Other(e.as_str().map_or_else(|| e.to_string(), str::to_owned))
                }),
            ),
        }
    }

    /// Opens an https page in the user's browser (links inside the window do
    /// not navigate it).
    pub fn open_url(&self, url: &str) {
        match self {
            Backend::Tauri => {
                let (b, url) = (self.clone(), url.to_owned());
                leptos::task::spawn_local(async move {
                    let _ = b.call::<()>(cmd::OPEN_URL, Some(url)).await;
                });
            }
            Backend::Demo(_) => {
                #[cfg(target_arch = "wasm32")]
                if let Some(w) = web_sys::window() {
                    let _ = w.open_with_url_and_target(url, "_blank");
                }
                #[cfg(not(target_arch = "wasm32"))]
                let _ = url;
            }
        }
    }

    pub async fn get_settings(&self) -> Result<AppSettings, CallError> {
        self.call(cmd::GET_SETTINGS, None::<()>).await
    }

    pub async fn save_settings(&self, s: &AppSettings) -> Result<(), CallError> {
        self.call(cmd::SAVE_SETTINGS, Some(s)).await
    }

    pub async fn crash_reports(&self) -> Result<CrashReports, CallError> {
        self.call(cmd::CRASH_REPORTS, None::<()>).await
    }

    /// A folder the user chooses; `None` when cancelled.
    pub async fn pick_folder(&self) -> Result<Option<String>, CallError> {
        self.call(cmd::PICK_FOLDER, None::<()>).await
    }

    pub async fn reveal(&self, path: &str) -> Result<(), CallError> {
        self.call(cmd::REVEAL, Some(path)).await
    }

    pub async fn mark_version(&self, label: &str) -> Result<(), CallError> {
        let request = MarkVersion {
            label: label.to_owned(),
        };
        self.call(cmd::MARK_VERSION, Some(request)).await
    }

    pub async fn home_state(&self) -> Result<HomeState, CallError> {
        self.call(cmd::HOME_STATE, None::<()>).await
    }

    pub async fn create_project(&self, name: &str) -> Result<OpenedProject, CallError> {
        let request = NewProject {
            name: name.to_owned(),
        };
        self.call(cmd::CREATE_PROJECT, Some(request)).await
    }

    pub async fn open_project(&self, id: &str) -> Result<OpenedProject, CallError> {
        self.call(cmd::OPEN_PROJECT, Some(id)).await
    }

    /// Takes a project off the list (`folder`: its folder goes to the trash too).
    pub async fn delete_project(&self, id: &str, folder: bool) -> Result<HomeState, CallError> {
        let request = DeleteProject {
            id: id.to_owned(),
            folder,
        };
        self.call(cmd::DELETE_PROJECT, Some(request)).await
    }

    pub async fn open_folder(&self) -> Result<Option<OpenedProject>, CallError> {
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

    pub async fn saved_connections(&self) -> Result<Vec<SavedConnection>, CallError> {
        self.call(cmd::SAVED_CONNECTIONS, None::<()>).await
    }

    pub async fn send_message(&self, m: &SendMessage) -> Result<(), CallError> {
        self.call(cmd::SEND_MESSAGE, Some(m)).await
    }

    pub async fn answer_confirmation(
        &self,
        request_id: &str,
        allow: bool,
    ) -> Result<(), CallError> {
        let a = Answer {
            request_id: request_id.to_owned(),
            answer: allow,
        };
        self.call(cmd::ANSWER_CONFIRMATION, Some(a)).await
    }

    pub async fn answer_plan(&self, request_id: &str, d: PlanDecision) -> Result<(), CallError> {
        let a = Answer {
            request_id: request_id.to_owned(),
            answer: d,
        };
        self.call(cmd::ANSWER_PLAN, Some(a)).await
    }

    pub async fn answer_question(&self, request_id: &str, text: &str) -> Result<(), CallError> {
        let a = Answer {
            request_id: request_id.to_owned(),
            answer: text.to_owned(),
        };
        self.call(cmd::ANSWER_QUESTION, Some(a)).await
    }

    pub async fn cancel_turn(&self) -> Result<(), CallError> {
        self.call(cmd::CANCEL_TURN, None::<()>).await
    }

    pub async fn continue_turn(&self) -> Result<(), CallError> {
        self.call(cmd::CONTINUE_TURN, None::<()>).await
    }

    pub async fn stuck_choice(&self, c: StuckChoice) -> Result<(), CallError> {
        self.call(cmd::STUCK_CHOICE, Some(c)).await
    }

    pub async fn resolve_interruption(&self, c: InterruptionChoice) -> Result<(), CallError> {
        self.call(cmd::RESOLVE_INTERRUPTION, Some(c)).await
    }

    async fn call<R: DeserializeOwned>(
        &self,
        command: &str,
        request: Option<impl Serialize>,
    ) -> Result<R, CallError> {
        let answer = self.call_raw(command, request).await;
        match answer {
            Ok(v) => serde_json::from_value(v).map_err(|e| CallError::Other(e.to_string())),
            Err(e) => Err(serde_json::from_value::<ConnectError>(e.clone())
                .map(CallError::Connect)
                .unwrap_or_else(|_| {
                    CallError::Other(e.as_str().map_or_else(|| e.to_string(), str::to_owned))
                })),
        }
    }

    /// The answer or the error as JSON.
    async fn call_raw(
        &self,
        command: &str,
        request: Option<impl Serialize>,
    ) -> Result<serde_json::Value, serde_json::Value> {
        let request = match request {
            Some(r) => Some(
                serde_json::to_value(r).map_err(|e| serde_json::Value::String(e.to_string()))?,
            ),
            None => None,
        };
        match self {
            #[cfg(target_arch = "wasm32")]
            Backend::Tauri => tauri::invoke(command, request).await,
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Tauri => {
                let _ = (command, request);
                Err(serde_json::Value::String(
                    "Tauri runs only in the window".into(),
                ))
            }
            Backend::Demo(demo) => match Demo::answer_slow(demo, command).await {
                Some(answer) => answer,
                None => Demo::answer(demo, command, request),
            },
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod tauri {
    //! `window.__TAURI_INTERNALS__` (`invoke`, `transformCallback`), reached
    //! through `Reflect` so no extern block (and no unsafe) is needed.

    use js_sys::{Function, JSON, Promise, Reflect};
    use serde_json::{Value, json};
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::{JsCast, JsValue};
    use wasm_bindgen_futures::JsFuture;

    fn internals() -> Option<JsValue> {
        let window = web_sys::window()?;
        Reflect::get(&window, &JsValue::from_str("__TAURI_INTERNALS__"))
            .ok()
            .filter(|v| v.is_object())
    }

    fn function(of: &JsValue, name: &str) -> Option<Function> {
        Reflect::get(of, &JsValue::from_str(name))
            .ok()
            .and_then(|f| f.dyn_into::<Function>().ok())
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

    async fn invoke_raw(command: &str, args: Value) -> Result<Value, Value> {
        let missing = || Value::String("not running inside AgentKnife".into());
        let internals = internals().ok_or_else(missing)?;
        let invoke = function(&internals, "invoke").ok_or_else(missing)?;
        let promise = invoke
            .call2(&internals, &JsValue::from_str(command), &to_js(&args))
            .map_err(|e| from_js(&e))?
            .dyn_into::<Promise>()
            .map_err(|e| from_js(&e))?;
        JsFuture::from(promise)
            .await
            .map(|v| from_js(&v))
            .map_err(|e| from_js(&e))
    }

    pub async fn invoke(command: &str, request: Option<Value>) -> Result<Value, Value> {
        let args = match request {
            Some(r) => json!({ "request": r }),
            None => json!({}),
        };
        invoke_raw(command, args).await
    }

    /// Like `@tauri-apps/api/event`'s `listen`: registers a callback id and asks
    /// the event plugin to call it for `event`.
    pub fn listen(event: &str, on_payload: impl Fn(Value) + 'static) {
        let Some(internals) = internals() else { return };
        let Some(transform) = function(&internals, "transformCallback") else {
            return;
        };
        let callback = Closure::<dyn Fn(JsValue)>::new(move |e: JsValue| {
            on_payload(from_js(&e)["payload"].take());
        });
        let Ok(id) = transform.call1(&internals, callback.as_ref()) else {
            return;
        };
        // The listener lives as long as the window.
        callback.forget();
        let args = json!({
            "event": event,
            "target": { "kind": "Any" },
            "handler": from_js(&id),
        });
        leptos::task::spawn_local(async move {
            let _ = invoke_raw("plugin:event|listen", args).await;
        });
    }
}

/// Waits `ms` milliseconds in the browser (the demo's pacing); no wait natively.
pub async fn sleep(ms: i32) {
    #[cfg(target_arch = "wasm32")]
    {
        let promise = js_sys::Promise::new(&mut |resolve, _| {
            if let Some(w) = web_sys::window() {
                let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
            }
        });
        let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = ms;
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
