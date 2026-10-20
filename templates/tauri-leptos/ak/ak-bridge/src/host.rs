//! Backend side: a Tauri plugin. With the `dev` feature and the `AK_BRIDGE_*`
//! environment set (AgentKnife's preview), it hides the app's windows and serves
//! the app's commands over a local WebSocket. Otherwise it does nothing.
//!
//! ```ignore
//! tauri::Builder::default()
//!     .plugin(ak_bridge::init())
//!     .invoke_handler(tauri::generate_handler![add_todo])
//! ```

use tauri::Runtime;
use tauri::plugin::{Builder, TauriPlugin};

/// The plugin. Register it before the app's own setup runs.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    let builder = Builder::new("ak-bridge");
    #[cfg(feature = "dev")]
    let builder = match server::Config::from_env() {
        Some(config) => builder
            .setup(move |app, _api| {
                let addr = server::start(app.clone(), config)?;
                // AgentKnife reads this line to learn the port.
                println!(
                    "{}{}",
                    crate::protocol::READY_PREFIX,
                    serde_json::json!({ "port": addr.port() })
                );
                Ok(())
            })
            // The preview shows the frontend inside AgentKnife; the app's own
            // windows only host the IPC.
            .on_window_ready(|w: tauri::Window<R>| {
                let _ = w.hide();
            }),
        None => builder,
    };
    builder.build()
}

/// Whether this process is AgentKnife's preview backend. Always `false` in
/// release builds; the app shows its windows when this is `false`.
pub fn is_preview() -> bool {
    #[cfg(feature = "dev")]
    {
        server::Config::from_env().is_some()
    }
    #[cfg(not(feature = "dev"))]
    {
        false
    }
}

#[cfg(feature = "dev")]
pub mod server {
    use std::collections::HashMap;
    use std::net::SocketAddr;
    use std::sync::{Arc, Mutex};

    use futures_util::{SinkExt, StreamExt};
    use serde_json::Value;
    use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponse, InvokeResponseBody};
    use tauri::webview::InvokeRequest;
    use tauri::{AppHandle, Manager, Runtime, Webview, WebviewWindow};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::mpsc;
    use tokio_tungstenite::tungstenite::Message;
    use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
    use tokio_tungstenite::tungstenite::http::StatusCode;

    use crate::protocol::{
        CONTROLLER_ROLE, ClientMessage, ControlAction, ENV_ORIGIN, ENV_PORT, ENV_TOKEN,
        PAGE_CLOSED, PAGE_NOT_OPEN, ServerMessage,
    };

    #[derive(Debug, Clone)]
    pub struct Config {
        pub port: u16,
        pub token: String,
        pub origin: Option<String>,
    }

    impl Config {
        /// `None` unless a token is set: no token, no server.
        pub fn from_env() -> Option<Config> {
            let token = std::env::var(ENV_TOKEN).ok().filter(|t| t.len() >= 16)?;
            Some(Config {
                port: std::env::var(ENV_PORT)
                    .ok()
                    .and_then(|p| p.parse().ok())
                    .unwrap_or(0),
                token,
                origin: std::env::var(ENV_ORIGIN).ok().filter(|o| !o.is_empty()),
            })
        }
    }

    type Tx = mpsc::UnboundedSender<ServerMessage>;

    /// Who is connected, and which control requests wait for the page.
    #[derive(Default)]
    struct Hub {
        /// Open preview pages by connection id; the newest one gets control.
        pages: Vec<(u64, Tx)>,
        /// Server control id -> (page connection, controller, controller's id).
        pending: HashMap<u64, (u64, Tx, u64)>,
        next: u64,
    }

    impl Hub {
        fn id(&mut self) -> u64 {
            self.next += 1;
            self.next
        }
    }

    type SharedHub = Arc<Mutex<Hub>>;

    /// Binds 127.0.0.1 and serves in the background. Returns the bound address.
    pub fn start<R: Runtime>(
        app: AppHandle<R>,
        config: Config,
    ) -> Result<SocketAddr, Box<dyn std::error::Error>> {
        let listener =
            tauri::async_runtime::block_on(TcpListener::bind(("127.0.0.1", config.port)))?;
        let addr = listener.local_addr()?;
        let config = Arc::new(config);
        let hub = SharedHub::default();
        tauri::async_runtime::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tauri::async_runtime::spawn(connection(
                    app.clone(),
                    stream,
                    config.clone(),
                    hub.clone(),
                ));
            }
        });
        Ok(addr)
    }

    fn reject(status: StatusCode) -> ErrorResponse {
        let mut r = ErrorResponse::new(None);
        *r.status_mut() = status;
        r
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Role {
        /// The preview page (runs control actions).
        Page,
        /// AgentKnife's agent (sends them).
        Controller,
    }

    /// The token (constant-time compare) and, if configured, the origin. Browsers
    /// always send `Origin`, so a missing one means a native client (AgentKnife).
    fn check(req: &Request, config: &Config) -> Result<Role, StatusCode> {
        let params: Vec<&str> = req
            .uri()
            .query()
            .into_iter()
            .flat_map(|q| q.split('&'))
            .collect();
        let token = params
            .iter()
            .find_map(|kv| kv.strip_prefix("token="))
            .unwrap_or_default();
        let same = token.len() == config.token.len()
            && token
                .bytes()
                .zip(config.token.bytes())
                .fold(0u8, |acc, (a, b)| acc | (a ^ b))
                == 0;
        if !same {
            return Err(StatusCode::UNAUTHORIZED);
        }
        let origin = req.headers().get("origin").and_then(|o| o.to_str().ok());
        if let (Some(want), Some(origin)) = (&config.origin, origin)
            && origin != want
        {
            return Err(StatusCode::FORBIDDEN);
        }
        Ok(if params.contains(&CONTROLLER_ROLE) {
            Role::Controller
        } else {
            Role::Page
        })
    }

    // tungstenite's handshake callback must return its (large) ErrorResponse.
    #[allow(clippy::result_large_err)]
    async fn connection<R: Runtime>(
        app: AppHandle<R>,
        stream: TcpStream,
        config: Arc<Config>,
        hub: SharedHub,
    ) {
        let mut role = Role::Page;
        let callback = |req: &Request, resp: Response| match check(req, &config) {
            Ok(r) => {
                role = r;
                Ok(resp)
            }
            Err(status) => Err(reject(status)),
        };
        let Ok(ws) = tokio_tungstenite::accept_hdr_async(stream, callback).await else {
            return;
        };
        let (mut sink, mut source) = ws.split();
        // Answers finish in any order; one writer sends them.
        let (tx, mut rx) = mpsc::unbounded_channel::<ServerMessage>();
        let writer = tauri::async_runtime::spawn(async move {
            while let Some(msg) = rx.recv().await {
                let Ok(text) = serde_json::to_string(&msg) else {
                    continue;
                };
                if sink.send(Message::text(text)).await.is_err() {
                    break;
                }
            }
        });
        let me = match hub.lock() {
            Ok(mut h) => {
                let id = h.id();
                if role == Role::Page {
                    h.pages.push((id, tx.clone()));
                }
                id
            }
            Err(_) => return,
        };
        while let Some(Ok(msg)) = source.next().await {
            let Message::Text(text) = msg else {
                continue;
            };
            match serde_json::from_str::<ClientMessage>(&text) {
                Ok(ClientMessage::Invoke { id, cmd, args }) => {
                    let tx = tx.clone();
                    dispatch(&app, cmd, args, move |ok, value| {
                        let _ = tx.send(ServerMessage::Result { id, ok, value });
                    });
                }
                Ok(ClientMessage::Control { id, action }) if role == Role::Controller => {
                    forward(&hub, &tx, id, action);
                }
                Ok(ClientMessage::ControlResult { id, ok, value }) if role == Role::Page => {
                    let waiting = hub.lock().ok().and_then(|mut h| h.pending.remove(&id));
                    if let Some((_, controller, their_id)) = waiting {
                        let _ = controller.send(ServerMessage::ControlResult {
                            id: their_id,
                            ok,
                            value,
                        });
                    }
                }
                Ok(ClientMessage::Control { id, .. }) => {
                    let _ = tx.send(ServerMessage::ControlResult {
                        id,
                        ok: false,
                        value: Value::String(
                            "only a controller connection may send control actions".into(),
                        ),
                    });
                }
                Ok(ClientMessage::ControlResult { .. }) => {}
                Err(e) => {
                    // Malformed request: answer with id 0 so the client can log it.
                    let _ = tx.send(ServerMessage::Result {
                        id: 0,
                        ok: false,
                        value: Value::String(format!("bad bridge message: {e}")),
                    });
                }
            }
        }
        if let Ok(mut h) = hub.lock() {
            h.pages.retain(|(id, _)| *id != me);
            // Nothing will answer what this page was asked.
            let lost: Vec<u64> = h
                .pending
                .iter()
                .filter(|(_, (page, _, _))| *page == me)
                .map(|(sid, _)| *sid)
                .collect();
            for sid in lost {
                if let Some((_, controller, their_id)) = h.pending.remove(&sid) {
                    let _ = controller.send(ServerMessage::ControlResult {
                        id: their_id,
                        ok: false,
                        value: Value::String(PAGE_CLOSED.into()),
                    });
                }
            }
        }
        drop(tx);
        let _ = writer.await;
    }

    /// Hands a controller's action to the newest open page.
    fn forward(hub: &SharedHub, controller: &Tx, their_id: u64, action: ControlAction) {
        let Ok(mut h) = hub.lock() else {
            return;
        };
        let Some((page_id, page)) = h.pages.last().cloned() else {
            let _ = controller.send(ServerMessage::ControlResult {
                id: their_id,
                ok: false,
                value: Value::String(PAGE_NOT_OPEN.into()),
            });
            return;
        };
        let sid = h.id();
        h.pending
            .insert(sid, (page_id, controller.clone(), their_id));
        let _ = page.send(ServerMessage::Control { id: sid, action });
    }

    /// The app's main webview, else any webview (for commands that take one).
    fn target<R: Runtime>(app: &AppHandle<R>) -> Option<Webview<R>> {
        if let Some(w) = app.get_webview_window("main") {
            return Some(w.as_ref().clone());
        }
        let mut all: Vec<(String, WebviewWindow<R>)> = app.webview_windows().into_iter().collect();
        all.sort_by(|a, b| a.0.cmp(&b.0));
        all.into_iter().next().map(|(_, w)| w.as_ref().clone())
    }

    /// Runs `cmd` through Tauri's own IPC on the main thread, exactly like a call
    /// from the app's webview, and reports `(ok, value)` once.
    pub fn dispatch<R: Runtime>(
        app: &AppHandle<R>,
        cmd: String,
        args: Value,
        reply: impl FnOnce(bool, Value) + Send + 'static,
    ) {
        let reply = Arc::new(Mutex::new(Some(reply)));
        let answer = {
            let reply = reply.clone();
            move |ok: bool, value: Value| {
                if let Some(f) = reply.lock().ok().and_then(|mut r| r.take()) {
                    f(ok, value);
                }
            }
        };
        let Some(webview) = target(app) else {
            answer(
                false,
                Value::String("the app has no window to run commands in".into()),
            );
            return;
        };
        let url = match webview.url() {
            Ok(u) => u,
            Err(e) => {
                answer(false, Value::String(format!("webview url: {e}")));
                return;
            }
        };
        let request = InvokeRequest {
            cmd,
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url,
            body: InvokeBody::Json(if args.is_null() {
                Value::Object(Default::default())
            } else {
                args
            }),
            headers: Default::default(),
            invoke_key: app.invoke_key().to_owned(),
        };
        let on_response = answer.clone();
        let sent = app.run_on_main_thread(move || {
            webview.on_message(
                request,
                Box::new(move |_webview, _cmd, response, _callback, _error| {
                    let (ok, value) = match response {
                        InvokeResponse::Ok(InvokeResponseBody::Json(json)) => {
                            (true, serde_json::from_str(&json).unwrap_or(Value::Null))
                        }
                        InvokeResponse::Ok(InvokeResponseBody::Raw(bytes)) => {
                            (true, Value::from(bytes))
                        }
                        InvokeResponse::Err(e) => (false, e.0),
                    };
                    on_response(ok, value);
                }),
            );
        });
        if let Err(e) = sent {
            answer(false, Value::String(format!("main thread: {e}")));
        }
    }
}
