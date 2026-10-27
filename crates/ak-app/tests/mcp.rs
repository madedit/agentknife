//! Other AI tools (MCP) through the real commands and host (mock runtime):
//! the setting starts and stops the listener, an allowed tool lists and calls
//! the tools, and without an open project the project tools say what to do.

#![allow(clippy::unwrap_used)]

use ak_app::state::AppState;
use ak_mcp::{Endpoint, HELLO};
use ak_project::Registry;
use ak_types::ipc::McpStatus;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ak_types::agent::AgentEvent;
use ak_types::ipc::{
    AGENT_EVENT, BUNDLE_FINISHED_EVENT, BUNDLE_STARTED_EVENT, DOCTOR_CHECKED_EVENT,
};
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ClientCapabilities, ClientConfig, Implementation,
    ReadResourceRequestParams,
};
use rmcp::service::{Peer, RunningService};
use rmcp::{RoleClient, ServiceExt};
use serde_json::{Value, json};
use tauri::test::{INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context};
use tauri::webview::InvokeRequest;
use tauri::{App, Listener, WebviewWindow, WebviewWindowBuilder};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn call(window: &WebviewWindow<MockRuntime>, cmd: &str, request: Value) -> Result<Value, Value> {
    let url = if cfg!(windows) {
        "http://tauri.localhost"
    } else {
        "tauri://localhost"
    };
    get_ipc_response(
        window,
        InvokeRequest {
            cmd: cmd.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: url.parse().unwrap(),
            body: tauri::ipc::InvokeBody::Json(json!({ "request": request })),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.into(),
        },
    )
    .map(|b| b.deserialize::<Value>().unwrap())
}

/// AgentKnife on the mock runtime with the setting on; Claude Code was
/// allowed before, so nobody is asked (the mock has no dialogs).
fn app_with_mcp(dir: &Path) -> (App<MockRuntime>, WebviewWindow<MockRuntime>, PathBuf) {
    let state_dir = dir.join("state");
    std::fs::create_dir_all(&state_dir).unwrap();
    std::fs::write(
        state_dir.join("settings.json"),
        json!({ "mcp_allowed": ["Claude Code"] }).to_string(),
    )
    .unwrap();
    let registry = Registry::load(&state_dir, &dir.join("Projects")).unwrap();
    let state = AppState::new(state_dir.clone(), registry, dir.join("toolchain")).unwrap();
    let app = ak_app::with_commands(mock_builder(), state)
        .build(mock_context(tauri::test::noop_assets()))
        .unwrap();
    let window = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let on: McpStatus =
        serde_json::from_value(call(&window, "set_mcp", json!(true)).unwrap()).unwrap();
    assert!(on.enabled && on.clients.is_empty());
    (app, window, state_dir)
}

/// Claude Code, connected through the endpoint file as `agentknife-mcp` does.
async fn connect(file: &Path) -> RunningService<RoleClient, ClientConfig> {
    let endpoint = Endpoint::read(file).unwrap();
    let (port, token) = (endpoint.port.unwrap(), endpoint.token.unwrap());
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    stream
        .write_all(format!("{HELLO} {token}\n").as_bytes())
        .await
        .unwrap();
    let mut answer = String::new();
    BufReader::new(&mut stream)
        .read_line(&mut answer)
        .await
        .unwrap();
    assert_eq!(answer, "OK\n");
    ClientConfig::new(
        ClientCapabilities::default(),
        Implementation::new("claude-code", "2.1.304"),
    )
    .serve(stream)
    .await
    .unwrap()
}

fn text(result: &rmcp::model::CallToolResult) -> String {
    serde_json::to_value(&result.content).unwrap()[0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn an_allowed_tool_gets_agentknifes_tools() {
    let dir = tempfile::tempdir().unwrap();
    let (_app, window, state_dir) = app_with_mcp(dir.path());
    let file = state_dir.join("mcp.json");

    tauri::async_runtime::block_on(async {
        let client = connect(&file).await;

        let tools = client.list_all_tools().await.unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
        for want in [
            "status",
            "list_projects",
            "open_project",
            "read_file",
            "write_file",
            "run_command",
            "read_knowledge",
            "build_status",
            "dom_summary",
            "interact",
        ] {
            assert!(names.contains(&want), "{want} missing in {names:?}");
        }
        assert!(
            !names.contains(&"ask_user"),
            "they ask their user themselves"
        );
        let read = tools.iter().find(|t| t.name == "read_file").unwrap();
        assert!(
            !read.input_schema["required"]
                .as_array()
                .unwrap()
                .contains(&json!("label")),
            "label is optional over MCP"
        );

        let status = client
            .call_tool(CallToolRequestParams::new("status"))
            .await
            .unwrap();
        assert!(
            text(&status).contains("No project is open"),
            "{}",
            text(&status)
        );
        let mut read = CallToolRequestParams::new("read_file");
        read.arguments = json!({ "path": "src/main.rs" }).as_object().cloned();
        let out = client.call_tool(read).await.unwrap();
        assert_eq!(out.is_error, Some(true));
        assert!(text(&out).contains("list_projects"));

        let listed: McpStatus =
            serde_json::from_value(call(&window, "mcp_status", json!(null)).unwrap()).unwrap();
        assert_eq!(listed.clients, ["Claude Code"]);

        // Off: the tool is disconnected and the file says so.
        let off: McpStatus =
            serde_json::from_value(call(&window, "set_mcp", json!(false)).unwrap()).unwrap();
        assert!(!off.enabled && off.clients.is_empty());
        assert!(
            client
                .call_tool(CallToolRequestParams::new("status"))
                .await
                .is_err()
        );
    });
    let off = Endpoint::read(&file).unwrap();
    assert!(!off.enabled && off.port.is_none());
}

/// A call with arguments, as a client makes it.
async fn tool(peer: &Peer<RoleClient>, name: &'static str, args: Value) -> CallToolResult {
    let mut call = CallToolRequestParams::new(name);
    call.arguments = args.as_object().cloned();
    peer.call_tool(call).await.unwrap()
}

/// Waits for the confirmation card of `tool` and answers it as the user.
async fn answer(
    window: &WebviewWindow<MockRuntime>,
    asked: &Mutex<Vec<(String, String)>>,
    tool: &str,
    yes: bool,
) {
    for _ in 0..600 {
        let id = asked
            .lock()
            .unwrap()
            .iter()
            .find(|(_, t)| t == tool)
            .map(|(id, _)| id.clone());
        if let Some(id) = id {
            asked.lock().unwrap().retain(|(i, _)| *i != id);
            call(
                window,
                "answer_confirmation",
                json!({ "request_id": id, "answer": yes }),
            )
            .unwrap();
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("{tool} never asked");
}

#[test]
fn the_workflow_tools_make_save_restore_and_ask_first() {
    let dir = tempfile::tempdir().unwrap();
    let (app, window, state_dir) = app_with_mcp(dir.path());
    // The confirmation cards the window would show.
    let asked = Arc::new(Mutex::new(Vec::new()));
    {
        let asked = asked.clone();
        app.listen_any(AGENT_EVENT, move |e| {
            if let Ok(AgentEvent::NeedsConfirmation {
                request_id, tool, ..
            }) = serde_json::from_str(e.payload())
            {
                asked.lock().unwrap().push((request_id, tool));
            }
        });
    }
    // What the window's tools and packaging cards are told.
    let cards = Arc::new(Mutex::new(Vec::new()));
    for name in [
        DOCTOR_CHECKED_EVENT,
        BUNDLE_STARTED_EVENT,
        BUNDLE_FINISHED_EVENT,
    ] {
        let cards = cards.clone();
        app.listen_any(name, move |_| cards.lock().unwrap().push(name));
    }

    tauri::async_runtime::block_on(async {
        let client = connect(&state_dir.join("mcp.json")).await;
        let peer = client.peer().clone();
        let names: Vec<String> = client
            .list_all_tools()
            .await
            .unwrap()
            .into_iter()
            .map(|t| t.name.into_owned())
            .collect();
        for want in [
            "new_project",
            "check_tools",
            "run_app",
            "package_app",
            "checkpoint",
            "list_versions",
            "restore",
            "get_log",
        ] {
            assert!(
                names.iter().any(|n| n == want),
                "{want} missing in {names:?}"
            );
        }

        // A new project opens at once (its preview waits for the tools).
        let made = tool(&peer, "new_project", json!({ "name": "記帳" })).await;
        assert_eq!(made.is_error, Some(false), "{}", text(&made));
        assert!(text(&made).contains("Project: 記帳"), "{}", text(&made));
        let root = PathBuf::from(
            text(&made)
                .lines()
                .next()
                .and_then(|l| {
                    l.split_once(" (")
                        .map(|(_, r)| r.trim_end_matches(')').to_owned())
                })
                .unwrap(),
        );
        assert!(
            root.join("Cargo.toml").is_file(),
            "the starter app is there"
        );

        // Documents: the knowledge pack and the project's notes.
        let docs = client.list_all_resources().await.unwrap();
        let uris: Vec<&str> = docs.iter().map(|d| d.uri.as_str()).collect();
        for want in [
            "agentknife://knowledge/overview",
            "agentknife://project/PROJECT.md",
            "agentknife://project/STATUS.md",
        ] {
            assert!(uris.contains(&want), "{want} missing in {uris:?}");
        }
        let notes = client
            .read_resource(ReadResourceRequestParams::new(
                "agentknife://project/PROJECT.md",
            ))
            .await
            .unwrap();
        let notes = serde_json::to_value(notes).unwrap();
        assert!(
            notes["contents"][0]["text"]
                .as_str()
                .unwrap()
                .contains("記帳")
        );

        // A change, saved as a version.
        let wrote = tool(
            &peer,
            "write_file",
            json!({ "path": "notes.txt", "content": "第一版" }),
        )
        .await;
        assert_eq!(wrote.is_error, Some(false), "{}", text(&wrote));
        let saved = tool(&peer, "checkpoint", json!({ "name": "第一版" })).await;
        assert!(
            text(&saved).starts_with("Saved version"),
            "{}",
            text(&saved)
        );
        let again = tool(&peer, "checkpoint", json!({})).await;
        assert!(
            text(&again).starts_with("Nothing changed"),
            "{}",
            text(&again)
        );
        let versions = text(&tool(&peer, "list_versions", json!({})).await);
        // Named after the tool and the step, in the UI language.
        assert!(
            versions.contains("Claude Code") && versions.contains("第一版"),
            "{versions}"
        );
        let initial = versions
            .lines()
            .find(|l| l.contains("| initial |"))
            .and_then(|l| l.trim_start_matches("- ").split(" | ").next())
            .unwrap()
            .to_owned();

        // Going back asks the user first: declined, then allowed.
        let back = {
            let peer = peer.clone();
            let initial = initial.clone();
            tauri::async_runtime::spawn(async move {
                tool(&peer, "restore", json!({ "version": initial })).await
            })
        };
        answer(&window, &asked, "restore", false).await;
        let back = back.await.unwrap();
        assert_eq!(back.is_error, Some(true));
        assert!(root.join("notes.txt").is_file(), "nothing changed");
        let back = {
            let peer = peer.clone();
            tauri::async_runtime::spawn(async move {
                tool(&peer, "restore", json!({ "version": initial })).await
            })
        };
        answer(&window, &asked, "restore", true).await;
        let back = back.await.unwrap();
        assert_eq!(back.is_error, Some(false), "{}", text(&back));
        assert!(!root.join("notes.txt").exists(), "back at the start");

        // Installing AgentKnife's tools waits for the user (declined here).
        let check = {
            let peer = peer.clone();
            tauri::async_runtime::spawn(async move { tool(&peer, "check_tools", json!({})).await })
        };
        answer(&window, &asked, "check_tools", false).await;
        let check = check.await.unwrap();
        assert_eq!(check.is_error, Some(true));
        assert!(text(&check).contains("did not want"), "{}", text(&check));

        // Without the tools the app cannot be run or packaged, and says so.
        for name in ["run_app", "package_app"] {
            let out = tool(&peer, name, json!({})).await;
            assert_eq!(out.is_error, Some(true), "{name}: {}", text(&out));
        }

        assert_eq!(
            *cards.lock().unwrap(),
            [
                DOCTOR_CHECKED_EVENT,
                BUNDLE_STARTED_EVENT,
                BUNDLE_FINISHED_EVENT
            ]
        );

        // The log has every step.
        let log = text(&tool(&peer, "get_log", json!({ "source": "ai", "lines": 400 })).await);
        for step in ["checkpoint", "restore", "check_tools", "package_app"] {
            assert!(log.contains(step), "{step} not in the log:\n{log}");
        }
        assert!(log.lines().all(|l| l.starts_with("[ai] ")), "{log}");
        let wrong = tool(&peer, "get_log", json!({ "source": "nope" })).await;
        assert_eq!(wrong.is_error, Some(true));

        client.cancel().await.unwrap();
    });
}
