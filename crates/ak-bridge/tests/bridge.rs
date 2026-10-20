//! The bridge server against a Tauri app on the mock runtime: commands run
//! through Tauri's own IPC (state, errors, unknown commands), and only clients
//! with the token (and origin) get in.

#![cfg(all(feature = "dev", not(target_arch = "wasm32")))]
// Test code: clippy's allow-unwrap-in-tests does not cover helper fns here.
#![allow(clippy::unwrap_used)]

use std::net::SocketAddr;
use std::sync::Mutex;

use ak_bridge::protocol::ServerMessage;
use ak_bridge::server::{self, Config};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};
use tauri::{App, State, WebviewWindowBuilder};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

const TOKEN: &str = "0123456789abcdef0123";

#[derive(Default)]
struct Todos(Mutex<Vec<String>>);

#[tauri::command]
fn add_todo(text: String, todos: State<'_, Todos>) -> Result<usize, String> {
    if text.trim().is_empty() {
        return Err("empty text".into());
    }
    let mut list = todos.0.lock().unwrap();
    list.push(text);
    Ok(list.len())
}

#[tauri::command]
async fn list_todos(todos: State<'_, Todos>) -> Result<Vec<String>, String> {
    Ok(todos.0.lock().unwrap().clone())
}

fn app() -> App<MockRuntime> {
    let app = mock_builder()
        .manage(Todos::default())
        .invoke_handler(tauri::generate_handler![add_todo, list_todos])
        .build(mock_context(noop_assets()))
        .unwrap();
    WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    app
}

fn start(app: &App<MockRuntime>, origin: Option<&str>) -> SocketAddr {
    server::start(
        app.handle().clone(),
        Config {
            port: 0,
            token: TOKEN.into(),
            origin: origin.map(Into::into),
        },
    )
    .unwrap()
}

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn connect(addr: SocketAddr, token: &str, origin: Option<&str>) -> Result<Ws, String> {
    let mut req = format!("ws://{addr}/?token={token}")
        .into_client_request()
        .unwrap();
    if let Some(o) = origin {
        req.headers_mut().insert("origin", o.parse().unwrap());
    }
    tokio_tungstenite::connect_async(req)
        .await
        .map(|(ws, _)| ws)
        .map_err(|e| e.to_string())
}

async fn call(ws: &mut Ws, id: u64, cmd: &str, args: Value) -> (bool, Value) {
    let msg = json!({ "type": "invoke", "id": id, "cmd": cmd, "args": args });
    ws.send(Message::text(msg.to_string())).await.unwrap();
    loop {
        let Some(Ok(Message::Text(text))) = ws.next().await else {
            panic!("connection closed");
        };
        let ServerMessage::Result { id: got, ok, value } = serde_json::from_str(&text).unwrap();
        if got == id {
            return (ok, value);
        }
    }
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn commands_run_through_tauri_ipc() {
    let app = app();
    let addr = start(&app, None);
    assert!(addr.ip().is_loopback());
    rt().block_on(async {
        let mut ws = connect(addr, TOKEN, None).await.unwrap();
        assert_eq!(
            call(&mut ws, 1, "add_todo", json!({ "text": "milk" })).await,
            (true, json!(1))
        );
        assert_eq!(
            call(&mut ws, 2, "add_todo", json!({ "text": "eggs" })).await,
            (true, json!(2))
        );
        // Managed state is the app's real state; async commands work too.
        assert_eq!(
            call(&mut ws, 3, "list_todos", Value::Null).await,
            (true, json!(["milk", "eggs"]))
        );
        // The command's own error value comes back as is.
        assert_eq!(
            call(&mut ws, 4, "add_todo", json!({ "text": " " })).await,
            (false, json!("empty text"))
        );
        // Missing arguments and unknown commands are Tauri's errors.
        let (ok, err) = call(&mut ws, 5, "add_todo", json!({})).await;
        assert!(!ok && err.to_string().contains("text"), "{err}");
        let (ok, err) = call(&mut ws, 6, "drop_database", json!({})).await;
        assert!(!ok && err.to_string().contains("drop_database"), "{err}");

        // A second preview tab shares the same backend.
        let mut other = connect(addr, TOKEN, None).await.unwrap();
        assert_eq!(
            call(&mut other, 1, "list_todos", Value::Null).await.1,
            json!(["milk", "eggs"])
        );
    });
}

#[test]
fn many_requests_in_flight() {
    let app = app();
    let addr = start(&app, None);
    rt().block_on(async {
        let mut ws = connect(addr, TOKEN, None).await.unwrap();
        for id in 1..=20 {
            let msg = json!({ "type": "invoke", "id": id, "cmd": "add_todo",
                              "args": { "text": format!("item {id}") } });
            ws.send(Message::text(msg.to_string())).await.unwrap();
        }
        let mut ids = Vec::new();
        while ids.len() < 20 {
            let Some(Ok(Message::Text(text))) = ws.next().await else {
                panic!("closed");
            };
            let ServerMessage::Result { id, ok, .. } = serde_json::from_str(&text).unwrap();
            assert!(ok);
            ids.push(id);
        }
        ids.sort_unstable();
        assert_eq!(ids, (1..=20).collect::<Vec<u64>>());
    });
}

#[test]
fn strangers_are_turned_away() {
    let app = app();
    let open = start(&app, None);
    let locked = start(&app, Some("http://127.0.0.1:8123"));
    rt().block_on(async {
        let err = connect(open, "wrong-token-wrong-token", None)
            .await
            .unwrap_err();
        assert!(err.contains("401"), "{err}");
        let err = connect(open, "", None).await.unwrap_err();
        assert!(err.contains("401"), "{err}");

        // Another web page in the user's browser, even with the token.
        let err = connect(locked, TOKEN, Some("https://evil.example"))
            .await
            .unwrap_err();
        assert!(err.contains("403"), "{err}");
        let err = connect(locked, TOKEN, None).await.unwrap_err();
        assert!(err.contains("403"), "{err}");
        assert!(
            connect(locked, TOKEN, Some("http://127.0.0.1:8123"))
                .await
                .is_ok()
        );
    });
}

#[test]
fn garbage_is_answered_not_fatal() {
    let app = app();
    let addr = start(&app, None);
    rt().block_on(async {
        let mut ws = connect(addr, TOKEN, None).await.unwrap();
        ws.send(Message::text("not json")).await.unwrap();
        let Some(Ok(Message::Text(text))) = ws.next().await else {
            panic!("closed");
        };
        let ServerMessage::Result { id, ok, .. } = serde_json::from_str(&text).unwrap();
        assert_eq!((id, ok), (0, false));
        assert_eq!(
            call(&mut ws, 1, "add_todo", json!({ "text": "still works" })).await,
            (true, json!(1))
        );
    });
}
