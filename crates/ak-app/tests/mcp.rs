//! Other AI tools (MCP) through the real commands and host (mock runtime):
//! the setting starts and stops the listener, an allowed tool lists and calls
//! the tools, and without an open project the project tools say what to do.

#![allow(clippy::unwrap_used)]

use ak_app::state::AppState;
use ak_mcp::{Endpoint, HELLO};
use ak_project::Registry;
use ak_types::ipc::McpStatus;
use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, ClientCapabilities, ClientConfig, Implementation};
use serde_json::{Value, json};
use tauri::test::{INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context};
use tauri::webview::InvokeRequest;
use tauri::{WebviewWindow, WebviewWindowBuilder};
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

fn text(result: &rmcp::model::CallToolResult) -> String {
    serde_json::to_value(&result.content).unwrap()[0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn an_allowed_tool_gets_agentknifes_tools() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join("state");
    std::fs::create_dir_all(&state_dir).unwrap();
    // Allowed before, so nobody is asked (the mock has no dialogs).
    std::fs::write(
        state_dir.join("settings.json"),
        json!({ "mcp_allowed": ["Claude Code"] }).to_string(),
    )
    .unwrap();
    let registry = Registry::load(&state_dir, &dir.path().join("Projects")).unwrap();
    let state = AppState::new(state_dir.clone(), registry, dir.path().join("toolchain")).unwrap();
    let app = ak_app::with_commands(mock_builder(), state)
        .build(mock_context(tauri::test::noop_assets()))
        .unwrap();
    let window = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();

    let on: McpStatus =
        serde_json::from_value(call(&window, "set_mcp", json!(true)).unwrap()).unwrap();
    assert!(on.enabled && on.clients.is_empty());
    let file = state_dir.join("mcp.json");
    let endpoint = Endpoint::read(&file).unwrap();
    let (port, token) = (endpoint.port.unwrap(), endpoint.token.unwrap());

    tauri::async_runtime::block_on(async {
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
        let client = ClientConfig::new(
            ClientCapabilities::default(),
            Implementation::new("claude-code", "2.1.304"),
        )
        .serve(stream)
        .await
        .unwrap();

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
