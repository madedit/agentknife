//! The server over a real local socket, against a fake host, with rmcp's client.

// Test code: clippy's allow-unwrap-in-tests does not cover helper fns here.
#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};

use ak_agent::tool::ToolOutput;
use ak_mcp::{Client, Document, Endpoint, HELLO, INSTRUCTIONS, McpHost, Server};
use ak_types::chat::ToolSpec;
use futures_util::future::BoxFuture;
use rmcp::ServiceExt;
use rmcp::model::{
    CallToolRequestParams, ClientCapabilities, ClientConfig, GetPromptRequestParams,
    Implementation, ReadResourceRequestParams,
};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

/// Client, tool, label, arguments.
type Call = (String, String, Option<String>, Value);

#[derive(Default)]
struct FakeHost {
    allow: bool,
    calls: Mutex<Vec<Call>>,
    seen: Mutex<Vec<String>>,
}

impl McpHost for FakeHost {
    fn tools(&self) -> Vec<ToolSpec> {
        vec![ToolSpec {
            name: "read_file".into(),
            description: "Read a file".into(),
            input_schema: json!({
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"],
            }),
        }]
    }

    fn call<'a>(
        &'a self,
        client: &'a Client,
        name: String,
        label: Option<String>,
        args: Value,
        _: CancellationToken,
    ) -> BoxFuture<'a, ToolOutput> {
        self.calls
            .lock()
            .unwrap()
            .push((client.name.clone(), name.clone(), label, args));
        Box::pin(async move {
            if name == "read_file" {
                ToolOutput::text("fn main() {}")
            } else {
                ToolOutput::error("no such tool")
            }
        })
    }

    fn connected<'a>(&'a self, client: &'a Client) -> BoxFuture<'a, bool> {
        self.seen.lock().unwrap().push(format!("+{}", client.name));
        let allow = self.allow;
        Box::pin(async move { allow })
    }

    fn disconnected(&self, client: &Client) {
        self.seen.lock().unwrap().push(format!("-{}", client.name));
    }

    fn documents(&self) -> BoxFuture<'_, Vec<Document>> {
        Box::pin(async {
            vec![Document {
                uri: OVERVIEW.into(),
                name: "knowledge/overview".into(),
                title: "Overview".into(),
                description: "Read first".into(),
                mime_type: "text/markdown",
            }]
        })
    }

    fn read<'a>(&'a self, uri: &'a str) -> BoxFuture<'a, Option<String>> {
        Box::pin(async move { (uri == OVERVIEW).then(|| "# Overview".to_owned()) })
    }
}

const OVERVIEW: &str = "agentknife://knowledge/overview";

async fn open(file: &std::path::Path, token: &str) -> (TcpStream, String) {
    let ep = Endpoint::read(file).unwrap();
    let mut s = TcpStream::connect(("127.0.0.1", ep.port.unwrap()))
        .await
        .unwrap();
    s.write_all(format!("{HELLO} {token}\n").as_bytes())
        .await
        .unwrap();
    let mut answer = String::new();
    BufReader::new(&mut s).read_line(&mut answer).await.unwrap();
    (s, answer)
}

fn claude() -> ClientConfig {
    ClientConfig::new(
        ClientCapabilities::default(),
        Implementation::new("claude-code", "2.1.304"),
    )
}

#[tokio::test]
async fn a_client_lists_and_calls_tools_through_the_host() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("mcp.json");
    let host = Arc::new(FakeHost {
        allow: true,
        ..FakeHost::default()
    });
    let server = Server::start(host.clone(), file.clone()).await.unwrap();
    let ep = Endpoint::read(&file).unwrap();
    assert!(ep.enabled && ep.port == Some(server.port()));
    let token = ep.token.unwrap();

    let (stream, answer) = open(&file, &token).await;
    assert_eq!(answer, "OK\n");
    let client = claude().serve(stream).await.unwrap();
    let info = client.peer_info().unwrap();
    assert_eq!(info.instructions.as_deref(), Some(INSTRUCTIONS));
    assert_eq!(
        info.server_info.as_ref().map(|s| s.name.as_str()),
        Some("agentknife")
    );

    let tools = client.list_all_tools().await.unwrap();
    assert_eq!(tools.len(), 1);
    assert!(tools[0].input_schema["properties"]["label"].is_object());

    let mut call = CallToolRequestParams::new("read_file");
    call.arguments = json!({ "path": "src/main.rs", "label": "讀取主程式" })
        .as_object()
        .cloned();
    let out = client.call_tool(call).await.unwrap();
    assert_eq!(out.is_error, Some(false));
    assert_eq!(
        serde_json::to_value(&out.content).unwrap()[0]["text"],
        "fn main() {}"
    );
    let out = client
        .call_tool(CallToolRequestParams::new("nope"))
        .await
        .unwrap();
    assert_eq!(out.is_error, Some(true));
    assert_eq!(
        host.calls.lock().unwrap()[0],
        (
            "Claude Code".to_owned(),
            "read_file".to_owned(),
            Some("讀取主程式".to_owned()),
            json!({ "path": "src/main.rs" })
        ),
        "the label is taken off the arguments"
    );

    client.cancel().await.unwrap();
    for _ in 0..50 {
        if host.seen.lock().unwrap().len() == 2 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(*host.seen.lock().unwrap(), ["+Claude Code", "-Claude Code"]);

    drop(server);
    let off = Endpoint::read(&file).unwrap();
    assert!(!off.enabled && off.port.is_none() && off.token.is_none());
}

#[tokio::test]
async fn a_wrong_token_or_a_declined_client_gets_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("mcp.json");
    let host = Arc::new(FakeHost::default()); // the user says no
    let _server = Server::start(host.clone(), file.clone()).await.unwrap();

    let (_, answer) = open(&file, "0123456789abcdef0123456789abcdef").await;
    assert_eq!(answer, "NO\n");

    let token = Endpoint::read(&file).unwrap().token.unwrap();
    let (stream, answer) = open(&file, &token).await;
    assert_eq!(answer, "OK\n");
    assert!(claude().serve(stream).await.is_err(), "initialize refused");
    assert!(host.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_client_without_a_handshake_is_asked_before_anything() {
    use rmcp::model::ProtocolVersion;
    use rmcp::service::{ClientLifecycleMode, ClientServiceExt};
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("mcp.json");
    let modern = || ClientLifecycleMode::Discover {
        preferred_versions: vec![ProtocolVersion::V_2026_07_28],
    };

    // The user says no: not even the list.
    let host = Arc::new(FakeHost::default());
    let server = Server::start(host.clone(), file.clone()).await.unwrap();
    let token = Endpoint::read(&file).unwrap().token.unwrap();
    let (stream, _) = open(&file, &token).await;
    let client = claude()
        .serve_with_lifecycle(stream, modern())
        .await
        .unwrap();
    assert!(client.list_all_tools().await.is_err());
    assert!(
        client
            .call_tool(CallToolRequestParams::new("read_file"))
            .await
            .is_err()
    );
    assert_eq!(*host.seen.lock().unwrap(), ["+Claude Code"], "asked once");
    assert!(host.calls.lock().unwrap().is_empty());
    drop(server);

    // The user says yes.
    let host = Arc::new(FakeHost {
        allow: true,
        ..FakeHost::default()
    });
    let _server = Server::start(host.clone(), file.clone()).await.unwrap();
    let token = Endpoint::read(&file).unwrap().token.unwrap();
    let (stream, _) = open(&file, &token).await;
    let client = claude()
        .serve_with_lifecycle(stream, modern())
        .await
        .unwrap();
    assert_eq!(client.list_all_tools().await.unwrap().len(), 1);
    let mut call = CallToolRequestParams::new("read_file");
    call.arguments = json!({ "path": "a.rs" }).as_object().cloned();
    assert_eq!(client.call_tool(call).await.unwrap().is_error, Some(false));
    assert_eq!(host.calls.lock().unwrap()[0].0, "Claude Code");
    client.cancel().await.unwrap();
    for _ in 0..50 {
        if host.seen.lock().unwrap().len() == 2 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(*host.seen.lock().unwrap(), ["+Claude Code", "-Claude Code"]);
}

#[tokio::test]
async fn documents_and_the_workflow_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("mcp.json");
    let host = Arc::new(FakeHost {
        allow: true,
        ..FakeHost::default()
    });
    let _server = Server::start(host.clone(), file.clone()).await.unwrap();
    let token = Endpoint::read(&file).unwrap().token.unwrap();
    let (stream, _) = open(&file, &token).await;
    let client = claude().serve(stream).await.unwrap();
    let caps = &client.peer_info().unwrap().capabilities;
    assert!(caps.resources.is_some() && caps.prompts.is_some());

    let docs = client.list_all_resources().await.unwrap();
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].uri, OVERVIEW);
    assert_eq!(docs[0].mime_type.as_deref(), Some("text/markdown"));
    let read = client
        .read_resource(ReadResourceRequestParams::new(OVERVIEW))
        .await
        .unwrap();
    let read = serde_json::to_value(&read).unwrap();
    assert_eq!(read["contents"][0]["text"], "# Overview");
    assert!(
        client
            .read_resource(ReadResourceRequestParams::new("agentknife://nothing"))
            .await
            .is_err()
    );

    let prompts = client.list_all_prompts().await.unwrap();
    assert_eq!(prompts.len(), 1);
    assert_eq!(prompts[0].name, ak_mcp::workflow::BUILD_APP);
    let mut get = GetPromptRequestParams::new(ak_mcp::workflow::BUILD_APP);
    get.arguments = json!({ "idea": "一個記帳 App" }).as_object().cloned();
    let prompt = serde_json::to_value(client.get_prompt(get).await.unwrap()).unwrap();
    let text = prompt["messages"][0]["content"]["text"].as_str().unwrap();
    assert!(
        text.contains("一個記帳 App") && text.contains("`check_tools`"),
        "{text}"
    );
    assert!(
        client
            .get_prompt(GetPromptRequestParams::new("other"))
            .await
            .is_err()
    );
}
