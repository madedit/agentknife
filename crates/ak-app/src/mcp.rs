//! Other AI tools (Claude Code, Codex, …) operating AgentKnife over MCP
//! (P1-MCP-001): the host `ak_mcp` calls, the listener's lifetime and the
//! window's commands.
//!
//! The tools are the built-in agent's ([`workbench`]): the same permission
//! policy and confirmation cards, each call shown as a step in the chat. Only
//! one AI changes the project at a time: while AgentKnife's own AI works,
//! changing tools are refused, and a turn does not start while a tool changes
//! the project ([`busy`]). Changes become a version once the tool has been
//! quiet for a minute (or leaves).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use ak_agent::Step;
use ak_agent::tool::{ToolContext, ToolOutput};
use ak_agent::tools::PreviewAccess;
use ak_mcp::{Client, Endpoint, McpHost, Server};
use ak_preview::profiles;
use ak_project::{CheckpointKind, Timeline};
use ak_types::agent::AgentEvent;
use ak_types::chat::{ContentPart, ToolCall, ToolSpec};
use ak_types::i18n::{I18n, args};
use ak_types::ipc::{
    AGENT_EVENT, MCP_EVENT, McpStatus, PROJECT_CHANGED_EVENT, PROJECT_OPENED_EVENT,
};
use ak_types::preview::{ControlAction, Picked, PreviewStatus};
use ak_types::settings::AppSettings;
use futures_util::future::BoxFuture;
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
use tokio_util::sync::CancellationToken;

use crate::state::{AppState, SettingsFile};
use crate::turn::{Workbench, log_step, workbench};
use crate::{Res, fail, ui_locale};

/// Quiet time after a tool's last change before its changes become a version.
const VERSION_AFTER: Duration = Duration::from_secs(60);

/// The listener and who is connected (in [`AppState`]).
#[derive(Default)]
pub struct McpState(Mutex<Clients>);

#[derive(Default)]
struct Clients {
    server: Option<Server>,
    clients: Vec<Client>,
    /// Tool calls changing the project right now.
    changing: usize,
    /// Changes not yet in a version, per client.
    unsaved: HashMap<u64, Unsaved>,
    next_step: u64,
}

struct Unsaved {
    root: PathBuf,
    /// The first step's label: names the version.
    label: String,
    /// Bumped by each change; the version waits for the last one.
    generation: u64,
}

impl McpState {
    fn with<T>(&self, f: impl FnOnce(&mut Clients) -> T) -> Option<T> {
        self.0.lock().ok().map(|mut r| f(&mut r))
    }
}

/// Another AI tool is changing the project (a turn must not start now).
pub fn busy(state: &AppState) -> bool {
    state.mcp.with(|r| r.changing > 0).unwrap_or(false)
}

/// `agentknife-mcp` next to AgentKnife.
pub fn program_path() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_owned();
    let name = if cfg!(windows) {
        "agentknife-mcp.exe"
    } else {
        "agentknife-mcp"
    };
    Some(dir.join(name)).filter(|p| p.is_file())
}

fn endpoint_file(state: &AppState) -> PathBuf {
    state.state_dir.join("mcp.json")
}

async fn status(state: &AppState) -> McpStatus {
    let (enabled, folder) = {
        let inner = state.inner.lock().await;
        (
            inner.settings.mcp_enabled,
            inner.registry.default_parent().display().to_string(),
        )
    };
    McpStatus {
        enabled,
        folder,
        clients: state
            .mcp
            .with(|r| r.clients.iter().map(|c| c.name.clone()).collect())
            .unwrap_or_default(),
        program: program_path()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
    }
}

async fn emit_status<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<AppState>();
    let _ = app.emit(MCP_EVENT, status(&state).await);
}

/// Listens while the setting is on; off, the endpoint file says so (the
/// program a tool starts then tells the user to turn it on).
pub async fn apply<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<AppState>();
    let enabled = state.inner.lock().await.settings.mcp_enabled;
    let file = endpoint_file(&state);
    let running = state.mcp.with(|r| r.server.is_some()).unwrap_or(false);
    if enabled && !running {
        match Server::start(Arc::new(Host::new(app.clone())), file).await {
            Ok(server) => {
                state.mcp.with(|r| r.server = Some(server));
            }
            Err(e) => tracing::warn!("MCP server did not start: {e}"),
        }
    } else if !enabled {
        // Dropping the server ends every connection and marks the file off.
        let server = state.mcp.with(|r| {
            r.clients.clear();
            r.server.take()
        });
        if server.flatten().is_none() {
            let _ = Endpoint {
                enabled: false,
                port: None,
                token: None,
            }
            .write(&file);
        }
    }
    emit_status(app).await;
}

/// AgentKnife closes: nobody listens any more. The file keeps whether the
/// setting is on, so a tool's `agentknife-mcp` starts AgentKnife only then
/// (every Claude Code session starts it, most have nothing to do here).
pub fn shut_down(state: &AppState) {
    // Dropping the server marks the file off first.
    state.mcp.with(|r| r.server.take());
    let enabled = state
        .inner
        .try_lock()
        .map(|i| i.settings.mcp_enabled)
        .unwrap_or(false);
    let _ = Endpoint {
        enabled,
        port: None,
        token: None,
    }
    .write(&endpoint_file(state));
}

#[tauri::command]
pub async fn mcp_status(state: State<'_, AppState>) -> Res<McpStatus> {
    Ok(status(&state).await)
}

/// A terminal in the projects folder, where the user sets up a tool and then
/// starts it (its setting is for that folder only).
#[tauri::command]
pub async fn open_terminal(state: State<'_, AppState>) -> Res<()> {
    let folder = state
        .inner
        .lock()
        .await
        .registry
        .default_parent()
        .to_path_buf();
    std::fs::create_dir_all(&folder).map_err(fail)?;
    terminal_in(&folder).map_err(fail)
}

fn terminal_in(dir: &std::path::Path) -> std::io::Result<()> {
    use std::process::{Command, Stdio};
    let quiet = |c: &mut Command| {
        c.current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
    };
    if cfg!(windows) {
        // Windows Terminal when there is one, else PowerShell in its own window.
        let mut wt = Command::new("wt.exe");
        wt.arg("-d").arg(dir);
        quiet(&mut wt);
        if wt.spawn().is_ok() {
            return Ok(());
        }
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "start", "", "powershell", "-NoExit"]);
        quiet(&mut cmd);
        return cmd.spawn().map(drop);
    }
    if cfg!(target_os = "macos") {
        let mut open = Command::new("open");
        open.args(["-a", "Terminal"]).arg(dir);
        quiet(&mut open);
        return open.spawn().map(drop);
    }
    // Linux: the desktop's default terminal, else the common ones.
    for program in [
        "x-terminal-emulator",
        "gnome-terminal",
        "konsole",
        "xfce4-terminal",
        "xterm",
    ] {
        let mut c = Command::new(program);
        quiet(&mut c);
        if c.spawn().is_ok() {
            return Ok(());
        }
    }
    Err(std::io::Error::other("no terminal program was found"))
}

#[tauri::command]
pub async fn set_mcp<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    request: bool,
) -> Res<McpStatus> {
    {
        let mut inner = state.inner.lock().await;
        inner.settings.mcp_enabled = request;
        inner.settings.save(&state.state_dir).map_err(fail)?;
    }
    apply(&app).await;
    Ok(status(&state).await)
}

fn words(id: &str, pairs: &[(&'static str, &str)]) -> String {
    let a = args(pairs.iter().map(|(k, v)| (*k, v.to_string())));
    I18n::new(ui_locale())
        .map(|i| i.t_args(id, &a))
        .unwrap_or_else(|_| id.to_owned())
}

/// What `ak_mcp` runs the tools through.
struct Host<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> Host<R> {
    fn new(app: AppHandle<R>) -> Self {
        Host { app }
    }

    fn state(&self) -> State<'_, AppState> {
        self.app.state::<AppState>()
    }
}

/// Tools of AgentKnife itself (not of a project).
const STATUS: &str = "status";
const LIST_PROJECTS: &str = "list_projects";
const OPEN_PROJECT: &str = "open_project";
/// Project tools other AI tools do not get: they ask their user themselves.
const HIDDEN: &[&str] = &["ask_user"];

fn app_tools() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: STATUS.into(),
            description: "What AgentKnife shows now: the open project (if any), whether its \
                          preview builds, whether AgentKnife's own AI is working, and the \
                          folders you may read besides the project. Call it first."
                .into(),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        ToolSpec {
            name: LIST_PROJECTS.into(),
            description: "The user's projects in AgentKnife, most recent first: id, name, \
                          folder."
                .into(),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        ToolSpec {
            name: OPEN_PROJECT.into(),
            description: "Opens one of the user's projects in AgentKnife (its window shows it \
                          and starts its preview). The project tools then work on it."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project": { "type": "string", "description": "Its id or name from list_projects." },
                },
                "required": ["project"],
            }),
        },
    ]
}

/// The project tools as they are listed: the same for every project.
fn project_tools(state: &AppState) -> &'static [ToolSpec] {
    static SPECS: OnceLock<Vec<ToolSpec>> = OnceLock::new();
    SPECS.get_or_init(|| {
        let bench = workbench(
            state,
            &std::env::temp_dir(),
            profiles::default_profile(),
            Arc::new(NoPreview),
            &AppSettings::default(),
            true,
        );
        bench
            .registry
            .specs()
            .into_iter()
            .filter(|s| !HIDDEN.contains(&s.name.as_str()))
            .map(|mut s| {
                // The registry adds the required `label`; MCP offers its own.
                if let Some(p) = s.input_schema["properties"].as_object_mut() {
                    p.remove(ak_agent::tool::LABEL_ARG);
                }
                if let Some(r) = s.input_schema["required"].as_array_mut() {
                    r.retain(|x| x != ak_agent::tool::LABEL_ARG);
                }
                s
            })
            .collect()
    })
}

impl<R: Runtime> McpHost for Host<R> {
    fn tools(&self) -> Vec<ToolSpec> {
        let mut all = app_tools();
        all.extend(project_tools(&self.state()).iter().cloned());
        all
    }

    fn call<'a>(
        &'a self,
        client: &'a Client,
        name: String,
        label: Option<String>,
        args: Value,
        cancel: CancellationToken,
    ) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            match name.as_str() {
                STATUS => self.status_text().await,
                LIST_PROJECTS => self.list_projects().await,
                OPEN_PROJECT => self.open_project(&args).await,
                n if HIDDEN.contains(&n) => {
                    ToolOutput::error(format!("There is no tool named `{n}`."))
                }
                _ => self.project_call(client, name, label, args, cancel).await,
            }
        })
    }

    fn connected<'a>(&'a self, client: &'a Client) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            let state = self.state();
            let (enabled, known) = {
                let inner = state.inner.lock().await;
                (
                    inner.settings.mcp_enabled,
                    inner.settings.mcp_allowed.contains(&client.name),
                )
            };
            if !enabled {
                return false;
            }
            if !known {
                if !self.ask(client).await {
                    return false;
                }
                let mut inner = state.inner.lock().await;
                inner.settings.mcp_allowed.push(client.name.clone());
                let _ = inner.settings.save(&state.state_dir);
            }
            state.mcp.with(|r| r.clients.push(client.clone()));
            emit_status(&self.app).await;
            true
        })
    }

    fn disconnected(&self, client: &Client) {
        let state = self.state();
        let unsaved = state.mcp.with(|r| {
            r.clients.retain(|c| c.id != client.id);
            r.unsaved.remove(&client.id)
        });
        let app = self.app.clone();
        tauri::async_runtime::spawn(async move {
            if let Some(Some(u)) = unsaved {
                save_version(&app, u).await;
            }
            emit_status(&app).await;
        });
    }
}

impl<R: Runtime> Host<R> {
    /// Asks the user, in front of everything, whether `client` may come in.
    async fn ask(&self, client: &Client) -> bool {
        if let Some(w) = self.app.get_webview_window("main") {
            let _ = w.unminimize();
            let _ = w.set_focus();
        }
        let name = client.name.as_str();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.app
            .dialog()
            .message(words("mcp-allow-body", &[("name", name)]))
            .title(words("mcp-allow-title", &[("name", name)]))
            .buttons(MessageDialogButtons::OkCancelCustom(
                words("mcp-allow-yes", &[]),
                words("mcp-allow-no", &[]),
            ))
            .show(move |yes| {
                let _ = tx.send(yes);
            });
        rx.await.unwrap_or(false)
    }

    async fn status_text(&self) -> ToolOutput {
        let state = self.state();
        let (open, settings) = {
            let inner = state.inner.lock().await;
            let open = inner.open.as_ref().map(|o| {
                let name = o
                    .project
                    .lock()
                    .map(|p| p.meta().name.clone())
                    .unwrap_or_default();
                (
                    name,
                    o.root.clone(),
                    o.preview.clone(),
                    o.cancel.is_some(),
                    crate::home::profile_of_root(o),
                )
            });
            (open, inner.settings.clone())
        };
        let Some((name, root, preview, working, profile)) = open else {
            return ToolOutput::text(
                "No project is open in AgentKnife. Call list_projects, then open_project.",
            );
        };
        let bench = workbench(&state, &root, profile, preview.clone(), &settings, true);
        let build = ak_agent::tools::preview::describe_status(&preview.status());
        ToolOutput::text(format!(
            "Project: {name} ({})\n{build}\nAgentKnife's own AI: {}{}",
            root.display(),
            if working {
                "working (changes are refused until it finishes)"
            } else {
                "idle"
            },
            bench.notes
        ))
    }

    async fn list_projects(&self) -> ToolOutput {
        let state = self.state();
        let inner = state.inner.lock().await;
        let rows: Vec<String> = inner
            .registry
            .recent()
            .iter()
            .map(|r| {
                format!(
                    "- {} | {} | {}{}",
                    r.entry.id,
                    r.entry.name,
                    r.entry.path.display(),
                    if r.exists { "" } else { " (folder missing)" }
                )
            })
            .collect();
        if rows.is_empty() {
            ToolOutput::text("The user has no projects yet. They make one in AgentKnife.")
        } else {
            ToolOutput::text(format!("id | name | folder\n{}", rows.join("\n")))
        }
    }

    async fn open_project(&self, args: &Value) -> ToolOutput {
        let Some(wanted) = args["project"].as_str().map(str::trim) else {
            return ToolOutput::error("`project` is required: an id or name from list_projects.");
        };
        let state = self.state();
        let id = {
            let inner = state.inner.lock().await;
            if inner.open.as_ref().is_some_and(|o| o.cancel.is_some()) {
                return ToolOutput::error(
                    "AgentKnife's own AI is working in the open project; try again when it \
                     has finished.",
                );
            }
            inner
                .registry
                .recent()
                .iter()
                .find(|r| r.entry.id == wanted || r.entry.name == wanted)
                .map(|r| r.entry.id.clone())
        };
        let Some(id) = id else {
            return ToolOutput::error(format!(
                "There is no project `{wanted}`. Call list_projects for the ids and names."
            ));
        };
        match crate::home::open_project_quietly(self.app.clone(), &state, id).await {
            Ok(opened) => {
                let _ = self.app.emit(PROJECT_OPENED_EVENT, opened);
                self.status_text().await
            }
            Err(e) => ToolOutput::error(format!("Could not open it: {e}")),
        }
    }

    async fn project_call(
        &self,
        client: &Client,
        name: String,
        label: Option<String>,
        args: Value,
        cancel: CancellationToken,
    ) -> ToolOutput {
        let state = self.state();
        let opened = {
            let inner = state.inner.lock().await;
            inner.open.as_ref().map(|o| {
                (
                    o.root.clone(),
                    o.preview.clone(),
                    o.log.clone(),
                    o.cancel.is_some(),
                    crate::home::profile_of_root(o),
                    inner.settings.clone(),
                )
            })
        };
        let Some((root, preview, log, working, profile, settings)) = opened else {
            return ToolOutput::error(
                "No project is open in AgentKnife. Call list_projects, then open_project.",
            );
        };
        let Workbench {
            registry,
            policy,
            references,
            bridge,
            ..
        } = workbench(&state, &root, profile, preview.clone(), &settings, true);
        let changes = registry.get(&name).is_some_and(|t| t.changes_project());
        if changes {
            if working {
                return ToolOutput::error(
                    "AgentKnife's own AI is working on this project right now. Wait until it \
                     has finished (see `status`), then try again.",
                );
            }
            state.mcp.with(|r| r.changing += 1);
        }
        let _done = Done(changes.then_some(&state.mcp));

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<AgentEvent>();
        let forward = {
            let app = self.app.clone();
            tauri::async_runtime::spawn(async move {
                while let Some(e) = rx.recv().await {
                    log_step(&log, &e);
                    let _ = app.emit(AGENT_EVENT, e);
                }
            })
        };
        let n = state
            .mcp
            .with(|r| {
                r.next_step += 1;
                r.next_step
            })
            .unwrap_or_default();
        let step_id = format!("mcp-{}-{n}", client.id);
        let shown = label.clone().unwrap_or_else(|| name.clone());
        let ctx = ToolContext {
            project_dir: Some(root.clone()),
            cancel,
            events: Some(tx.clone()),
            reference_dirs: references,
        };
        let call = ToolCall {
            id: step_id.clone(),
            name,
            arguments: args,
            invalid_arguments: None,
        };
        let out = Step {
            tools: &registry,
            gate: &policy,
            confirmer: bridge.as_ref(),
            events: &tx,
            ctx: &ctx,
        }
        .run(
            &call,
            &step_id,
            Some(words(
                "mcp-step",
                &[("name", client.name.as_str()), ("label", shown.as_str())],
            )),
            |_| None,
        )
        .await
        .unwrap_or_else(|| ToolOutput::error("Cancelled."));
        drop(ctx);
        drop(tx);
        let _ = forward.await;

        if !out.touched.is_empty() {
            let mut touched = out.touched.clone();
            // Ids the user can point at, as after a turn of the built-in AI.
            for rel in &out.touched {
                let path = root.join(rel);
                if let Ok(src) = std::fs::read_to_string(&path)
                    && let Some(new) = profile.add_missing_ids(rel, &src)
                    && std::fs::write(&path, new).is_ok()
                    && !touched.contains(rel)
                {
                    touched.push(rel.clone());
                }
            }
            let _ = preview.files_changed(&touched).await;
            let _ = self.app.emit(PROJECT_CHANGED_EVENT, ());
            self.later_version(client, root, shown);
        }
        out
    }

    /// The changes become a version once `client` has been quiet a while.
    fn later_version(&self, client: &Client, root: PathBuf, label: String) {
        let state = self.state();
        let name = client.name.clone();
        let Some(generation) = state.mcp.with(|r| {
            let u = r.unsaved.entry(client.id).or_insert_with(|| Unsaved {
                root,
                label: words(
                    "mcp-step",
                    &[("name", name.as_str()), ("label", label.as_str())],
                ),
                generation: 0,
            });
            u.generation += 1;
            u.generation
        }) else {
            return;
        };
        let app = self.app.clone();
        let id = client.id;
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(VERSION_AFTER).await;
            let state = app.state::<AppState>();
            let due = state
                .mcp
                .with(|r| match r.unsaved.get(&id) {
                    Some(u) if u.generation == generation => r.unsaved.remove(&id),
                    _ => None,
                })
                .flatten();
            if let Some(u) = due {
                save_version(&app, u).await;
            }
        });
    }
}

async fn save_version<R: Runtime>(app: &AppHandle<R>, u: Unsaved) {
    let Unsaved { root, label, .. } = u;
    let _ = tokio::task::spawn_blocking(move || {
        Timeline::open(&root).and_then(|mut t| t.checkpoint(CheckpointKind::Turn, &label, None))
    })
    .await;
    let _ = app.emit(PROJECT_CHANGED_EVENT, ());
}

/// Ends a changing call's count, however the call ends.
struct Done<'a>(Option<&'a McpState>);

impl Drop for Done<'_> {
    fn drop(&mut self) {
        if let Some(m) = self.0 {
            m.with(|r| r.changing = r.changing.saturating_sub(1));
        }
    }
}

/// For listing the tools: a preview that is not running.
struct NoPreview;

impl PreviewAccess for NoPreview {
    fn status(&self) -> PreviewStatus {
        PreviewStatus::default()
    }
    fn settled(&self, _: Duration) -> BoxFuture<'_, PreviewStatus> {
        Box::pin(async { PreviewStatus::default() })
    }
    fn files_changed<'a>(&'a self, _: &'a [String]) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }
    fn act(&self, _: ControlAction) -> BoxFuture<'_, Result<Value, String>> {
        Box::pin(async { Err("no preview".to_owned()) })
    }
    fn selection(&self) -> Option<Picked> {
        None
    }
    fn can_capture(&self) -> bool {
        false
    }
    fn screenshot(&self, _: Option<String>) -> BoxFuture<'_, Result<ContentPart, String>> {
        Box::pin(async { Err("no preview".to_owned()) })
    }
}
