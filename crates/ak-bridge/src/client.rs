//! Frontend side (wasm): one `invoke` for the app's whole life.
//!
//! - Inside the real app (`__TAURI_INTERNALS__` exists): Tauri IPC.
//! - In AgentKnife's preview (`dev` feature, `?ak_bridge=<ws url>`): the bridge
//!   WebSocket, which runs the same commands in the app's backend process.

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

#[derive(Debug, Clone, PartialEq)]
pub enum InvokeError {
    /// Neither Tauri nor the preview bridge is available (e.g. opened in a
    /// normal browser).
    NoBackend,
    /// The command returned an error (its error value).
    Command(Value),
    /// The connection or the JavaScript call failed.
    Transport(String),
    /// The result did not have the expected type.
    Decode(String),
}

impl std::fmt::Display for InvokeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InvokeError::NoBackend => {
                f.write_str("no app backend (not running in Tauri or the preview)")
            }
            InvokeError::Command(v) => match v.as_str() {
                Some(s) => f.write_str(s),
                None => write!(f, "{v}"),
            },
            InvokeError::Transport(e) => write!(f, "bridge: {e}"),
            InvokeError::Decode(e) => write!(f, "unexpected result: {e}"),
        }
    }
}

impl std::error::Error for InvokeError {}

/// Calls the Tauri command `cmd` with `args` (an object whose fields are the
/// command's parameters, camelCase as in Tauri's JavaScript API).
pub async fn invoke<T: DeserializeOwned>(
    cmd: &str,
    args: &impl Serialize,
) -> Result<T, InvokeError> {
    let args = serde_json::to_value(args).map_err(|e| InvokeError::Decode(e.to_string()))?;
    let value = invoke_value(cmd, args).await?;
    serde_json::from_value(value).map_err(|e| InvokeError::Decode(e.to_string()))
}

/// [`invoke`] without types.
pub async fn invoke_value(cmd: &str, args: Value) -> Result<Value, InvokeError> {
    if let Some(internals) = tauri_internals() {
        return tauri_invoke(&internals, cmd, args).await;
    }
    #[cfg(feature = "dev")]
    if let Some(url) = bridge::url() {
        return bridge::invoke(&url, cmd, args).await;
    }
    Err(InvokeError::NoBackend)
}

fn tauri_internals() -> Option<JsValue> {
    let window = web_sys::window()?;
    js_sys::Reflect::get(&window, &JsValue::from_str("__TAURI_INTERNALS__"))
        .ok()
        .filter(|v| v.is_object())
}

fn to_js(v: &Value) -> Result<JsValue, InvokeError> {
    js_sys::JSON::parse(&v.to_string()).map_err(|e| InvokeError::Transport(js_text(&e)))
}

fn from_js(v: &JsValue) -> Value {
    if v.is_undefined() || v.is_null() {
        return Value::Null;
    }
    js_sys::JSON::stringify(v)
        .ok()
        .and_then(|s| s.as_string())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null)
}

fn js_text(v: &JsValue) -> String {
    v.as_string()
        .or_else(|| js_sys::JSON::stringify(v).ok().and_then(|s| s.as_string()))
        .unwrap_or_else(|| "unknown error".into())
}

async fn tauri_invoke(internals: &JsValue, cmd: &str, args: Value) -> Result<Value, InvokeError> {
    let f: js_sys::Function = js_sys::Reflect::get(internals, &JsValue::from_str("invoke"))
        .ok()
        .and_then(|f| f.dyn_into().ok())
        .ok_or_else(|| InvokeError::Transport("__TAURI_INTERNALS__.invoke is missing".into()))?;
    let promise: js_sys::Promise = f
        .call2(internals, &JsValue::from_str(cmd), &to_js(&args)?)
        .map_err(|e| InvokeError::Transport(js_text(&e)))?
        .dyn_into()
        .map_err(|_| InvokeError::Transport("invoke did not return a promise".into()))?;
    match JsFuture::from(promise).await {
        Ok(v) => Ok(from_js(&v)),
        // Tauri rejects with the command's error value.
        Err(e) => Err(InvokeError::Command(from_js(&e))),
    }
}

#[cfg(feature = "dev")]
mod bridge {
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;
    use std::rc::Rc;

    use futures_channel::oneshot;
    use serde_json::Value;
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;
    use web_sys::{MessageEvent, WebSocket};

    use super::InvokeError;
    use crate::protocol::{ClientMessage, STORAGE_KEY, ServerMessage, URL_PARAM};

    type Pending = RefCell<HashMap<u64, oneshot::Sender<Result<Value, InvokeError>>>>;
    type Handler = Closure<dyn FnMut(web_sys::Event)>;

    struct Conn {
        ws: WebSocket,
        open: Cell<bool>,
        /// Requests sent before the socket opened.
        queue: RefCell<Vec<String>>,
        pending: Pending,
        next_id: Cell<u64>,
        // Kept alive as long as the connection.
        _handlers: RefCell<Vec<Handler>>,
    }

    thread_local! {
        static CONN: RefCell<Option<Rc<Conn>>> = const { RefCell::new(None) };
    }

    /// The bridge URL: from `?ak_bridge=` on first load, then from sessionStorage
    /// (in-app navigation may drop the query).
    pub fn url() -> Option<String> {
        let window = web_sys::window()?;
        let storage = window.session_storage().ok().flatten();
        let from_query = window
            .location()
            .search()
            .ok()
            .and_then(|s| web_sys::UrlSearchParams::new_with_str(&s).ok())
            .and_then(|p| p.get(URL_PARAM));
        if let Some(url) = from_query {
            if let Some(s) = &storage {
                let _ = s.set_item(STORAGE_KEY, &url);
            }
            return Some(url);
        }
        storage.and_then(|s| s.get_item(STORAGE_KEY).ok().flatten())
    }

    fn fail_all(conn: &Conn, why: &str) {
        for (_, tx) in conn.pending.borrow_mut().drain() {
            let _ = tx.send(Err(InvokeError::Transport(why.to_owned())));
        }
    }

    fn connect(url: &str) -> Result<Rc<Conn>, InvokeError> {
        let ws = WebSocket::new(url).map_err(|e| InvokeError::Transport(super::js_text(&e)))?;
        let conn = Rc::new(Conn {
            ws,
            open: Cell::new(false),
            queue: RefCell::new(Vec::new()),
            pending: RefCell::new(HashMap::new()),
            next_id: Cell::new(1),
            _handlers: RefCell::new(Vec::new()),
        });
        let weak = Rc::downgrade(&conn);
        let on_open = Handler::new(move |_| {
            if let Some(c) = weak.upgrade() {
                c.open.set(true);
                for text in c.queue.borrow_mut().drain(..) {
                    let _ = c.ws.send_with_str(&text);
                }
            }
        });
        let weak = Rc::downgrade(&conn);
        let on_message = Handler::new(move |e: web_sys::Event| {
            let (Some(c), Some(text)) = (
                weak.upgrade(),
                e.dyn_ref::<MessageEvent>()
                    .and_then(|m| m.data().as_string()),
            ) else {
                return;
            };
            let Ok(ServerMessage::Result { id, ok, value }) = serde_json::from_str(&text) else {
                return;
            };
            if let Some(tx) = c.pending.borrow_mut().remove(&id) {
                let _ = tx.send(if ok {
                    Ok(value)
                } else {
                    Err(InvokeError::Command(value))
                });
            }
        });
        let weak = Rc::downgrade(&conn);
        let on_close = Handler::new(move |_| {
            if let Some(c) = weak.upgrade() {
                fail_all(&c, "the preview backend disconnected");
            }
            // The next invoke reconnects (the backend restarts after code changes).
            CONN.with(|slot| *slot.borrow_mut() = None);
        });
        conn.ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
        conn.ws
            .set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        conn.ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));
        conn.ws.set_onerror(Some(on_close.as_ref().unchecked_ref()));
        conn._handlers
            .borrow_mut()
            .extend([on_open, on_message, on_close]);
        Ok(conn)
    }

    pub async fn invoke(url: &str, cmd: &str, args: Value) -> Result<Value, InvokeError> {
        let conn = CONN.with(|slot| -> Result<Rc<Conn>, InvokeError> {
            let mut slot = slot.borrow_mut();
            if let Some(c) = slot.as_ref() {
                return Ok(c.clone());
            }
            let c = connect(url)?;
            *slot = Some(c.clone());
            Ok(c)
        })?;
        let id = conn.next_id.get();
        conn.next_id.set(id + 1);
        let text = serde_json::to_string(&ClientMessage::Invoke {
            id,
            cmd: cmd.to_owned(),
            args,
        })
        .map_err(|e| InvokeError::Decode(e.to_string()))?;
        let (tx, rx) = oneshot::channel();
        conn.pending.borrow_mut().insert(id, tx);
        if conn.open.get() {
            conn.ws
                .send_with_str(&text)
                .map_err(|e| InvokeError::Transport(super::js_text(&e)))?;
        } else {
            conn.queue.borrow_mut().push(text);
        }
        drop(conn);
        rx.await
            .unwrap_or_else(|_| Err(InvokeError::Transport("the request was dropped".into())))
    }
}
