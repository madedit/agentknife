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
    connect_as(addr, &format!("token={token}"), origin).await
}

async fn connect_as(addr: SocketAddr, query: &str, origin: Option<&str>) -> Result<Ws, String> {
    let mut req = format!("ws://{addr}/?{query}")
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
        let ServerMessage::Result { id: got, ok, value } = serde_json::from_str(&text).unwrap()
        else {
            continue;
        };
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
            let ServerMessage::Result { id, ok, .. } = serde_json::from_str(&text).unwrap() else {
                panic!("not a result: {text}");
            };
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
        // No Origin at all: a native client (AgentKnife's controller), not a page.
        assert!(connect(locked, TOKEN, None).await.is_ok());
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
        let ServerMessage::Result { id, ok, .. } = serde_json::from_str(&text).unwrap() else {
            panic!("not a result: {text}");
        };
        assert_eq!((id, ok), (0, false));
        assert_eq!(
            call(&mut ws, 1, "add_todo", json!({ "text": "still works" })).await,
            (true, json!(1))
        );
    });
}

async fn next_msg(ws: &mut Ws) -> ServerMessage {
    loop {
        match ws.next().await {
            Some(Ok(Message::Text(text))) => return serde_json::from_str(&text).unwrap(),
            Some(Ok(_)) => continue,
            other => panic!("connection ended: {other:?}"),
        }
    }
}

async fn control(ws: &mut Ws, id: u64, action: Value) {
    let msg = json!({ "type": "control", "id": id, "action": action });
    ws.send(Message::text(msg.to_string())).await.unwrap();
}

#[test]
fn control_actions_reach_the_newest_page_and_answers_come_back() {
    use ak_bridge::protocol::ControlAction;
    let app = app();
    let addr = start(&app, Some("http://127.0.0.1:8123"));
    rt().block_on(async {
        let page_origin = Some("http://127.0.0.1:8123");
        let controller_q = format!("token={TOKEN}&role=controller");
        let mut agent = connect_as(addr, &controller_q, None).await.unwrap();

        // No page yet.
        control(&mut agent, 1, json!({ "action": "dom_summary" })).await;
        assert_eq!(
            next_msg(&mut agent).await,
            ServerMessage::ControlResult {
                id: 1,
                ok: false,
                value: json!("the preview page is not open")
            }
        );

        // Two pages: the newest (e.g. after a reload) gets the action.
        let mut old_page = connect(addr, TOKEN, page_origin).await.unwrap();
        let mut page = connect(addr, TOKEN, page_origin).await.unwrap();
        control(
            &mut agent,
            7,
            json!({ "action": "click", "ak_id": "add-button" }),
        )
        .await;
        let ServerMessage::Control { id: sid, action } = next_msg(&mut page).await else {
            panic!("expected a control action");
        };
        assert_eq!(
            action,
            ControlAction::Click {
                ak_id: "add-button".into()
            }
        );
        let reply = json!({ "type": "control_result", "id": sid, "ok": true,
                            "value": { "clicked": "add-button", "tag": "button" } });
        page.send(Message::text(reply.to_string())).await.unwrap();
        assert_eq!(
            next_msg(&mut agent).await,
            ServerMessage::ControlResult {
                id: 7,
                ok: true,
                value: json!({ "clicked": "add-button", "tag": "button" })
            }
        );

        // The page closes before answering: the agent hears about it.
        control(&mut agent, 8, json!({ "action": "app_state" })).await;
        let ServerMessage::Control { .. } = next_msg(&mut page).await else {
            panic!("expected a control action");
        };
        page.close(None).await.unwrap();
        assert_eq!(
            next_msg(&mut agent).await,
            ServerMessage::ControlResult {
                id: 8,
                ok: false,
                value: json!("the preview page closed")
            }
        );

        // The older page takes over.
        control(&mut agent, 9, json!({ "action": "dom_summary" })).await;
        let ServerMessage::Control { action, .. } = next_msg(&mut old_page).await else {
            panic!("expected a control action");
        };
        assert_eq!(action, ControlAction::DomSummary);

        // A page cannot drive another page.
        control(&mut old_page, 3, json!({ "action": "dom_summary" })).await;
        let ServerMessage::ControlResult {
            id: 3, ok: false, ..
        } = next_msg(&mut old_page).await
        else {
            panic!("a page must not send control actions");
        };

        // The controller may also call commands.
        assert_eq!(
            call(&mut agent, 10, "add_todo", json!({ "text": "via agent" })).await,
            (true, json!(1))
        );
    });
}
