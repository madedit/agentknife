//! The socket AgentKnife listens on and the MCP side of each connection.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ak_agent::tool::{LABEL_ARG, ToolOutput};
use ak_types::chat::{ContentPart, ToolSpec};
use futures_util::future::BoxFuture;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    InitializeRequestParams, InitializeResult, ListToolsResult, PaginatedRequestParams,
    ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;

use crate::endpoint::{Endpoint, random_token};
use crate::{HELLO, INSTRUCTIONS};

/// One connected client (one `agentknife-mcp` process).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Client {
    /// Unique while AgentKnife runs.
    pub id: u64,
    /// What the user knows it as: "Claude Code", "Codex", …
    pub name: String,
}

/// What AgentKnife does for the clients (implemented by ak-app).
pub trait McpHost: Send + Sync + 'static {
    /// The tools offered, in a stable order, without the `label` argument.
    fn tools(&self) -> Vec<ToolSpec>;

    /// Runs a tool for `client`. `label` is the client's description of the
    /// step, if it gave one. `cancel` fires when the client gives up.
    fn call<'a>(
        &'a self,
        client: &'a Client,
        name: String,
        label: Option<String>,
        args: Value,
        cancel: CancellationToken,
    ) -> BoxFuture<'a, ToolOutput>;

    /// A client finished connecting: whether the user lets it in (asked once
    /// per client name).
    fn connected<'a>(&'a self, client: &'a Client) -> BoxFuture<'a, bool>;

    fn disconnected(&self, client: &Client);
}

/// AgentKnife's listener. Writes the endpoint file when started; dropping it
/// stops listening and marks the file off.
pub struct Server {
    port: u16,
    stop: CancellationToken,
    file: PathBuf,
}

impl Server {
    /// Listens on 127.0.0.1 (a free port) with a new random token.
    pub async fn start(host: Arc<dyn McpHost>, file: PathBuf) -> std::io::Result<Server> {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).await?;
        let port = listener.local_addr()?.port();
        let token = random_token()?;
        Endpoint {
            enabled: true,
            port: Some(port),
            token: Some(token.clone()),
        }
        .write(&file)?;
        let stop = CancellationToken::new();
        let ids = Arc::new(AtomicU64::new(1));
        tokio::spawn({
            let stop = stop.clone();
            async move {
                loop {
                    let stream = tokio::select! {
                        _ = stop.cancelled() => return,
                        accepted = listener.accept() => match accepted {
                            Ok((stream, _)) => stream,
                            Err(_) => continue,
                        },
                    };
                    let id = ids.fetch_add(1, Ordering::Relaxed);
                    tokio::spawn(connection(
                        stream,
                        token.clone(),
                        host.clone(),
                        id,
                        stop.clone(),
                    ));
                }
            }
        });
        Ok(Server { port, stop, file })
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.cancel();
        let _ = Endpoint {
            enabled: false,
            port: None,
            token: None,
        }
        .write(&self.file);
    }
}

/// How long a new connection has to say hello.
const HELLO_WAIT: Duration = Duration::from_secs(5);

async fn connection(
    stream: TcpStream,
    token: String,
    host: Arc<dyn McpHost>,
    id: u64,
    stop: CancellationToken,
) {
    let (read, mut write) = stream.into_split();
    // Read only the hello line; whatever follows it is already MCP and stays
    // in the buffer for the transport.
    let mut read = BufReader::new(read);
    let mut line = String::new();
    let hello = tokio::time::timeout(HELLO_WAIT, (&mut read).take(256).read_line(&mut line)).await;
    if !matches!(hello, Ok(Ok(n)) if n > 0) || !hello_ok(&line, &token) {
        let _ = write.write_all(b"NO\n").await;
        return;
    }
    if write.write_all(b"OK\n").await.is_err() {
        return;
    }
    let admitted = Arc::new(Mutex::new(None));
    let handler = Handler {
        host: host.clone(),
        id,
        admission: tokio::sync::Mutex::new(Admission::Pending),
        admitted: admitted.clone(),
    };
    // Initialize runs inside `serve` for older clients; newer ones are let in
    // on their first request, so who connected is known only afterwards.
    if let Ok(running) = handler.serve((read, write)).await {
        tokio::select! {
            _ = stop.cancelled() => {}
            _ = running.waiting() => {}
        }
    }
    let client = admitted.lock().ok().and_then(|c| c.clone());
    if let Some(c) = client {
        host.disconnected(&c);
    }
}

/// `AGENTKNIFE-MCP/1 <token>`; the token compared without an early exit.
fn hello_ok(line: &str, token: &str) -> bool {
    let Some(given) = line
        .trim_end()
        .strip_prefix(HELLO)
        .and_then(|r| r.strip_prefix(' '))
    else {
        return false;
    };
    given.len() == token.len()
        && given
            .bytes()
            .zip(token.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

struct Handler {
    host: Arc<dyn McpHost>,
    id: u64,
    /// Held while the user is asked, so a second request waits for the answer.
    admission: tokio::sync::Mutex<Admission>,
    /// The client once let in (for `disconnected`).
    admitted: Arc<Mutex<Option<Client>>>,
}

enum Admission {
    Pending,
    In(Client),
    Out,
}

impl Handler {
    /// Lets the client in on its first request: `initialize` (protocols up to
    /// 2025-11-25), or any request from a 2026-07-28 client, which has no
    /// handshake and names itself in each request's metadata. Asked once per
    /// connection; nothing is listed or run before the user agrees.
    async fn admit(&self, info: Option<Implementation>) -> Result<Client, ErrorData> {
        let mut admission = self.admission.lock().await;
        match &*admission {
            Admission::In(c) => return Ok(c.clone()),
            Admission::Out => return Err(refused()),
            Admission::Pending => {}
        }
        let client = Client {
            id: self.id,
            name: info
                .as_ref()
                .map_or_else(|| "AI tool".to_owned(), display_name),
        };
        if !self.host.connected(&client).await {
            *admission = Admission::Out;
            return Err(refused());
        }
        if let Ok(mut a) = self.admitted.lock() {
            *a = Some(client.clone());
        }
        *admission = Admission::In(client.clone());
        Ok(client)
    }
}

fn refused() -> ErrorData {
    ErrorData::invalid_request(
        "The user did not allow this tool to operate AgentKnife.",
        None,
    )
}

/// The name a person knows a client by.
pub fn display_name(info: &Implementation) -> String {
    if let Some(title) = info.title.as_deref().filter(|t| !t.trim().is_empty()) {
        return title.trim().to_owned();
    }
    let name = info.name.trim();
    match name.to_ascii_lowercase().as_str() {
        "claude-code" | "claude code" => "Claude Code".to_owned(),
        n if n.starts_with("codex") => "Codex".to_owned(),
        n if n.starts_with("gemini") => "Gemini CLI".to_owned(),
        "" => "AI tool".to_owned(),
        _ => name.to_owned(),
    }
}

/// A tool as clients see it: `label` is offered but optional.
pub fn mcp_tool(spec: ToolSpec) -> Tool {
    let mut schema = match spec.input_schema {
        Value::Object(m) => m,
        _ => serde_json::Map::new(),
    };
    schema.insert("type".into(), json!("object"));
    let props = schema.entry("properties").or_insert_with(|| json!({}));
    if let Some(p) = props.as_object_mut() {
        p.insert(
            LABEL_ARG.into(),
            json!({
                "type": "string",
                "description": "Optional: a few plain words in the user's language saying what \
                                this step does, shown to the user in AgentKnife.",
            }),
        );
    }
    Tool::new(spec.name, spec.description, Arc::new(schema))
}

fn mcp_result(out: ToolOutput) -> CallToolResult {
    let content = out
        .content
        .into_iter()
        .map(|c| match c {
            ContentPart::Text { text } => ContentBlock::text(text),
            ContentPart::Image {
                media_type,
                data_base64,
            } => ContentBlock::image(data_base64, media_type),
        })
        .collect();
    if out.is_error {
        CallToolResult::error(content)
    } else {
        CallToolResult::success(content)
    }
}

impl ServerHandler for Handler {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("agentknife", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }

    async fn initialize(
        &self,
        request: InitializeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, ErrorData> {
        context.peer.set_peer_info(request.clone());
        self.admit(Some(request.client_info.clone())).await?;
        self.negotiate_initialize(&request)
    }

    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        self.admit(context.client_info()).await?;
        Ok(ListToolsResult::with_all_items(
            self.host.tools().into_iter().map(mcp_tool).collect(),
        ))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let client = self.admit(context.client_info()).await?;
        let mut args = Value::Object(request.arguments.unwrap_or_default());
        let label = args
            .as_object_mut()
            .and_then(|a| a.remove(LABEL_ARG))
            .and_then(|l| l.as_str().map(str::trim).map(str::to_owned))
            .filter(|l| !l.is_empty());
        let out = self
            .host
            .call(&client, request.name.into_owned(), label, args, context.ct)
            .await;
        Ok(mcp_result(out).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_needs_the_exact_token() {
        assert!(hello_ok("AGENTKNIFE-MCP/1 abc123\n", "abc123"));
        assert!(hello_ok("AGENTKNIFE-MCP/1 abc123\r\n", "abc123"));
        assert!(!hello_ok("AGENTKNIFE-MCP/1 abc124\n", "abc123"));
        assert!(!hello_ok("AGENTKNIFE-MCP/1 abc12\n", "abc123"));
        assert!(!hello_ok("AGENTKNIFE-MCP/1abc123\n", "abc123"));
        assert!(!hello_ok("POST / HTTP/1.1\r\n", "abc123"));
    }

    #[test]
    fn clients_get_names_people_know() {
        let n = |name: &str| display_name(&Implementation::new(name, "1"));
        assert_eq!(n("claude-code"), "Claude Code");
        assert_eq!(n("codex-mcp-client"), "Codex");
        assert_eq!(n("gemini-cli-mcp-client"), "Gemini CLI");
        assert_eq!(n("my-tool"), "my-tool");
        let mut titled = Implementation::new("x", "1");
        titled.title = Some("Cursor".into());
        assert_eq!(display_name(&titled), "Cursor");
    }

    #[test]
    fn label_is_offered_but_optional() {
        let tool = mcp_tool(ToolSpec {
            name: "read_file".into(),
            description: "Read a file".into(),
            input_schema: json!({
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"],
            }),
        });
        let schema = Value::Object((*tool.input_schema).clone());
        assert!(schema["properties"]["label"].is_object());
        assert_eq!(schema["required"], json!(["path"]));
    }
}
