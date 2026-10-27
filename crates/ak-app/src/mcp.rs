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
use ak_agent::permission::{ConfirmRequest, Confirmer};
use ak_agent::tool::{ToolContext, ToolOutput};
use ak_agent::tools::PreviewAccess;
use ak_mcp::{Client, Document, Endpoint, McpHost, Server};
use ak_preview::profiles;
use ak_project::{AGENTKNIFE_DIR, CheckpointKind, Timeline};
use ak_types::agent::{AgentEvent, ConfirmKind, ConfirmReason};
use ak_types::chat::{ContentPart, ToolCall, ToolSpec};
use ak_types::doctor::{CheckScope, CheckStatus};
use ak_types::i18n::{I18n, args};
use ak_types::ipc::{
    AGENT_EVENT, BUNDLE_FINISHED_EVENT, BUNDLE_STARTED_EVENT, BundleFailure, DOCTOR_CHECKED_EVENT,
    DoctorCheck, MCP_EVENT, McpStatus, PROJECT_CHANGED_EVENT, PROJECT_OPENED_EVENT,
};
use ak_types::log::LogSource;
use ak_types::preview::{ControlAction, Picked, PreviewStatus};
use ak_types::settings::AppSettings;
use ak_types::stack::StackProfile;
use futures_util::future::BoxFuture;
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
use tokio_util::sync::CancellationToken;

use crate::state::{AppState, SettingsFile};
use crate::turn::{UiBridge, Workbench, log_step, workbench};
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
const NEW_PROJECT: &str = "new_project";
const CHECK_TOOLS: &str = "check_tools";
const RUN_APP: &str = "run_app";
const PACKAGE_APP: &str = "package_app";
const CHECKPOINT: &str = "checkpoint";
const LIST_VERSIONS: &str = "list_versions";
const RESTORE: &str = "restore";
const GET_LOG: &str = "get_log";
/// The tools above that are steps in the chat.
const APP_STEPS: &[&str] = &[
    NEW_PROJECT,
    CHECK_TOOLS,
    RUN_APP,
    PACKAGE_APP,
    CHECKPOINT,
    LIST_VERSIONS,
    RESTORE,
    GET_LOG,
];
/// `get_log`: lines by default and at most.
const LOG_LINES: usize = 80;
const LOG_MAX: usize = 400;
/// Characters of a step's output shown in its details.
const DETAIL_CHARS: usize = 2_000;
const NO_PROJECT: &str =
    "No project is open in AgentKnife. Call list_projects, then open_project (or new_project).";
const OWN_AI_WORKING: &str = "AgentKnife's own AI is working on this project right now. Wait \
                              until it has finished (see `status`), then try again.";
/// Documents (MCP resources).
const KNOWLEDGE_URI: &str = "agentknife://knowledge/";
const PROJECT_URI: &str = "agentknife://project/";
/// The project's notes a client may read: name, file under `.agentknife/`, what it is.
const NOTES: &[(&str, &str, &str)] = &[
    (
        "PROJECT.md",
        "PROJECT.md",
        "What the app is for and what was decided; kept up to date for the next session.",
    ),
    (
        "STATUS.md",
        "plan/STATUS.md",
        "What is done, what is being worked on, what is next.",
    ),
];
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
        ToolSpec {
            name: NEW_PROJECT.into(),
            description: "Makes a new project with the starter app and opens it in AgentKnife \
                          (its preview starts). Agree on the name with the user first."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "A short name for the app, in the user's language." },
                },
                "required": ["name"],
            }),
        },
        ToolSpec {
            name: CHECK_TOOLS.into(),
            description: "Checks that everything needed to build apps is on this computer. \
                          AgentKnife's own tools are installed once the user agrees in its \
                          window (minutes, a few GB); parts of the system the user installs \
                          themselves, with the steps this returns. Call it before the first \
                          build on a new computer, and again after the user installed something."
                .into(),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        ToolSpec {
            name: RUN_APP.into(),
            description: "Opens the app in its own window, as it runs once installed (the \
                          preview is a test page), so the user can try it. Returns once it \
                          runs, or with the build output when it did not start."
                .into(),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        ToolSpec {
            name: PACKAGE_APP.into(),
            description: "Packages the app into one file the user can keep and share (a few \
                          minutes), and returns where it is and the steps to install and start \
                          it, which you pass on to the user."
                .into(),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        ToolSpec {
            name: CHECKPOINT.into(),
            description: "Marks the project as it is now as a finished part the user can go \
                          back to (kept for good). Call it after each part that works. \
                          AgentKnife also saves your changes by itself a minute after the \
                          last one; this names a working state."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "What the version has, in a few words in the user's language (\"login page works\")." },
                },
            }),
        },
        ToolSpec {
            name: LIST_VERSIONS.into(),
            description: "The project's versions, newest first: id, kind, name, changed files."
                .into(),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        ToolSpec {
            name: RESTORE.into(),
            description: "Takes the project back to an earlier version, after the user agrees \
                          in AgentKnife's window. The work before it stays a version too."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "version": { "type": "string", "description": "An id from list_versions." },
                },
                "required": ["version"],
            }),
        },
        ToolSpec {
            name: GET_LOG.into(),
            description: "The last lines of the open project's log: the preview's builds \
                          (`screens`, `backend`), the real window (`window`), packaging \
                          (`bundle`) and the steps taken (`ai`)."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "source": { "type": "string", "enum": ["screens", "backend", "window", "bundle", "ai"], "description": "Only this part of the log." },
                    "lines": { "type": "integer", "minimum": 1, "maximum": LOG_MAX, "description": "How many lines (default 80)." },
                },
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
                n if APP_STEPS.contains(&n) => self.app_step(client, n, label, args, cancel).await,
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

    fn documents(&self) -> BoxFuture<'_, Vec<Document>> {
        Box::pin(async move {
            let (profile, root) = self.open_profile().await;
            let mut docs: Vec<Document> = profile
                .knowledge_pack()
                .topics
                .iter()
                .map(|t| Document {
                    uri: format!("{KNOWLEDGE_URI}{}", t.id),
                    name: format!("knowledge/{}", t.id),
                    title: t.title.into(),
                    description: t.summary.into(),
                    mime_type: MARKDOWN,
                })
                .collect();
            if let Some(root) = root {
                for (name, rel, about) in NOTES {
                    if root.join(AGENTKNIFE_DIR).join(rel).is_file() {
                        docs.push(Document {
                            uri: format!("{PROJECT_URI}{name}"),
                            name: format!("project/{name}"),
                            title: format!("The project's {name}"),
                            description: (*about).into(),
                            mime_type: MARKDOWN,
                        });
                    }
                }
            }
            docs
        })
    }

    fn read<'a>(&'a self, uri: &'a str) -> BoxFuture<'a, Option<String>> {
        Box::pin(async move {
            let (profile, root) = self.open_profile().await;
            if let Some(id) = uri.strip_prefix(KNOWLEDGE_URI) {
                return profile
                    .knowledge_pack()
                    .topic(id)
                    .map(|t| t.body.to_owned());
            }
            let name = uri.strip_prefix(PROJECT_URI)?;
            let (_, rel, _) = NOTES.iter().find(|(n, _, _)| *n == name)?;
            std::fs::read_to_string(root?.join(AGENTKNIFE_DIR).join(rel)).ok()
        })
    }
}

const MARKDOWN: &str = "text/markdown";
/// `list_versions` shows this many.
const VERSIONS_SHOWN: usize = 30;

/// The ids of what is missing (or the wrong version) in `scope`.
fn missing(check: &DoctorCheck, scope: CheckScope) -> Vec<String> {
    check
        .report
        .items
        .iter()
        .filter(|i| i.status != CheckStatus::Ok && i.scope == scope)
        .map(|i| i.id.clone())
        .collect()
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
        let build = ak_agent::tools::preview::describe_status(&preview.status())
            + &ak_agent::tools::preview::describe_size(preview.page_size());
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

    /// The open project's stack and folder (the default stack without one).
    async fn open_profile(&self) -> (&'static dyn StackProfile, Option<PathBuf>) {
        let state = self.state();
        let inner = state.inner.lock().await;
        match inner.open.as_ref() {
            Some(o) => (crate::home::profile_of_root(o), Some(o.root.clone())),
            None => (profiles::default_profile(), None),
        }
    }

    /// The open project's folder, unless AgentKnife's own AI is working in it.
    async fn idle_project(&self) -> Result<PathBuf, ToolOutput> {
        let state = self.state();
        let inner = state.inner.lock().await;
        match inner.open.as_ref() {
            None => Err(ToolOutput::error(NO_PROJECT)),
            Some(o) if o.cancel.is_some() => Err(ToolOutput::error(OWN_AI_WORKING)),
            Some(o) => Ok(o.root.clone()),
        }
    }

    fn step_id(&self, client: &Client) -> String {
        let n = self
            .state()
            .mcp
            .with(|r| {
                r.next_step += 1;
                r.next_step
            })
            .unwrap_or_default();
        format!("mcp-{}-{n}", client.id)
    }

    /// Shows a step's progress in the chat and the project's log.
    async fn show(&self, event: AgentEvent) {
        let log = {
            let state = self.state();
            let inner = state.inner.lock().await;
            inner.open.as_ref().map(|o| o.log.clone())
        };
        if let Some(log) = log {
            log_step(&log, &event);
        }
        let _ = self.app.emit(AGENT_EVENT, event);
    }

    /// Asks the user with a confirmation card in the chat, as a project tool's
    /// risky step does.
    async fn confirm(
        &self,
        step: &str,
        tool: &str,
        label: &str,
        kind: ConfirmKind,
        detail: String,
    ) -> bool {
        let reason = ConfirmReason { kind, detail };
        let request = ConfirmRequest {
            request_id: format!("{step}-confirm"),
            tool: tool.into(),
            label: label.into(),
            reason: reason.clone(),
        };
        self.show(AgentEvent::NeedsConfirmation {
            request_id: request.request_id.clone(),
            tool: tool.into(),
            label: label.into(),
            reason,
        })
        .await;
        UiBridge(self.state().pending.clone())
            .confirm(request)
            .await
    }

    /// One of AgentKnife's own tools (not a project tool), shown as a step.
    async fn app_step(
        &self,
        client: &Client,
        name: &str,
        label: Option<String>,
        args: Value,
        cancel: CancellationToken,
    ) -> ToolOutput {
        let step = self.step_id(client);
        let shown = label.unwrap_or_else(|| name.to_owned());
        let title = words(
            "mcp-step",
            &[("name", client.name.as_str()), ("label", shown.as_str())],
        );
        let detail = match &args {
            Value::Object(m) if !m.is_empty() => args.to_string(),
            _ => String::new(),
        };
        self.show(AgentEvent::StepStarted {
            step_id: step.clone(),
            tool: name.into(),
            label: title.clone(),
            detail,
        })
        .await;
        let out = tokio::select! {
            biased;
            _ = cancel.cancelled() => ToolOutput::error("Cancelled."),
            out = self.app_tool(client, name, &step, &title, &args) => out,
        };
        let text: String = out
            .content
            .iter()
            .filter_map(|c| match c {
                ContentPart::Text { text } => Some(text.as_str()),
                ContentPart::Image { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
            .chars()
            .take(DETAIL_CHARS)
            .collect();
        self.show(AgentEvent::StepFinished {
            step_id: step,
            ok: !out.is_error,
            output: text,
        })
        .await;
        out
    }

    async fn app_tool(
        &self,
        client: &Client,
        name: &str,
        step: &str,
        title: &str,
        args: &Value,
    ) -> ToolOutput {
        match name {
            NEW_PROJECT => self.new_project(args).await,
            CHECK_TOOLS => self.check_tools(step, title).await,
            RUN_APP => self.run_app().await,
            PACKAGE_APP => self.package_app().await,
            CHECKPOINT => self.checkpoint(client, title, args).await,
            LIST_VERSIONS => self.list_versions().await,
            RESTORE => self.restore(client, step, title, args).await,
            GET_LOG => self.get_log(args).await,
            n => ToolOutput::error(format!("There is no tool named `{n}`.")),
        }
    }

    async fn new_project(&self, args: &Value) -> ToolOutput {
        let Some(name) = args["name"]
            .as_str()
            .map(str::trim)
            .filter(|n| !n.is_empty())
        else {
            return ToolOutput::error("`name` is required: a short name agreed with the user.");
        };
        let state = self.state();
        if state
            .inner
            .lock()
            .await
            .open
            .as_ref()
            .is_some_and(|o| o.cancel.is_some())
        {
            return ToolOutput::error(OWN_AI_WORKING);
        }
        match crate::home::create_project_quietly(self.app.clone(), &state, name).await {
            Ok(opened) => {
                let _ = self.app.emit(PROJECT_OPENED_EVENT, opened);
                self.status_text().await
            }
            Err(e) => ToolOutput::error(format!("Could not make it: {e}")),
        }
    }

    async fn check_tools(&self, step: &str, title: &str) -> ToolOutput {
        let check = match crate::tools::doctor_check(self.state()).await {
            Ok(c) => c,
            Err(e) => return ToolOutput::error(format!("The check failed: {e}")),
        };
        let _ = self.app.emit(DOCTOR_CHECKED_EVENT, &check);
        if check.report.ready() {
            return ToolOutput::text(
                "Everything AgentKnife needs to build apps is on this computer.",
            );
        }
        let own = missing(&check, CheckScope::Isolated);
        let check = if own.is_empty() {
            check
        } else {
            let names: Vec<String> = own
                .iter()
                .map(|id| words(&format!("doctor-item-{id}"), &[]))
                .collect();
            if !self
                .confirm(
                    step,
                    CHECK_TOOLS,
                    title,
                    ConfirmKind::Download,
                    names.join(", "),
                )
                .await
            {
                return ToolOutput::error(format!(
                    "The user did not want AgentKnife to install its tools now ({}). Apps \
                     cannot be built without them: ask when it suits them.",
                    own.join(", ")
                ));
            }
            match crate::tools::doctor_install(self.app.clone(), self.state()).await {
                Ok(c) => c,
                Err(e) => return ToolOutput::error(format!("The install failed: {e}")),
            }
        };
        let _ = self.app.emit(DOCTOR_CHECKED_EVENT, &check);
        if check.report.ready() {
            return ToolOutput::text(
                "AgentKnife installed its tools; everything needed to build apps is there. \
                 The preview starts by itself.",
            );
        }
        let mut out = String::new();
        let failed: Vec<String> = check
            .report
            .items
            .iter()
            .filter(|i| i.status != CheckStatus::Ok && i.scope == CheckScope::Isolated)
            .map(|i| format!("- {}: {}", i.id, i.detail.as_deref().unwrap_or("not there")))
            .collect();
        if !failed.is_empty() {
            out.push_str(&format!(
                "Some of AgentKnife's own tools did not install:\n{}\n",
                failed.join("\n")
            ));
        }
        let system = missing(&check, CheckScope::System);
        if !system.is_empty() {
            out.push_str(&format!(
                "The user installs these parts of the system themselves: {}. AgentKnife's \
                 window shows them the steps (with an Install button where it can start the \
                 official installer); then they click \"Check again\". Tell them in plain \
                 words, and call check_tools again when they are done. The steps as the \
                 window shows them:\n",
                system.join(", ")
            ));
            for g in &check.guides {
                for s in &g.steps {
                    out.push_str(&format!("- {}", words(&s.message, &[])));
                    if let Some(c) = &s.command {
                        out.push_str(&format!("\n  {c}"));
                    }
                    if let Some(l) = &s.link {
                        out.push_str(&format!("\n  {l}"));
                    }
                    out.push('\n');
                }
            }
        }
        ToolOutput::error(out.trim_end().to_owned())
    }

    async fn run_app(&self) -> ToolOutput {
        if let Err(refused) = self.idle_project().await {
            return refused;
        }
        match crate::workspace::run_app(self.state()).await {
            Ok(()) => ToolOutput::text(
                "The app is open in its own window, as it runs once installed (its data \
                 stays apart from an installed copy). The user can try it there; calling \
                 run_app again replaces that window.",
            ),
            Err(e) => ToolOutput::error(format!("The app did not start:\n{e}")),
        }
    }

    async fn package_app(&self) -> ToolOutput {
        if let Err(refused) = self.idle_project().await {
            return refused;
        }
        let _ = self.app.emit(BUNDLE_STARTED_EVENT, ());
        let result = crate::tools::bundle(self.app.clone(), self.state())
            .await
            .map_err(|v| {
                serde_json::from_value::<BundleFailure>(v).unwrap_or(BundleFailure {
                    kind: "other".into(),
                    detail: String::new(),
                })
            });
        let _ = self.app.emit(BUNDLE_FINISHED_EVENT, &result);
        match result {
            Ok(r) => {
                let a = [("file", r.file.as_str()), ("app", r.app.as_str())];
                let steps: Vec<String> = r
                    .steps
                    .iter()
                    .enumerate()
                    .map(|(n, s)| {
                        let mut line = format!("{}. {}", n + 1, words(&s.message, &a));
                        if let Some(c) = &s.command {
                            line.push_str(&format!("\n   {c}"));
                        }
                        line
                    })
                    .collect();
                ToolOutput::text(format!(
                    "Packaged: {}\nPass these steps on to the user, in their language:\n{}",
                    r.path,
                    steps.join("\n")
                ))
            }
            Err(f) => ToolOutput::error(format!("Packaging failed ({}).\n{}", f.kind, f.detail)),
        }
    }

    async fn checkpoint(&self, client: &Client, title: &str, args: &Value) -> ToolOutput {
        let root = match self.idle_project().await {
            Ok(r) => r,
            Err(refused) => return refused,
        };
        let label = match args["name"]
            .as_str()
            .map(str::trim)
            .filter(|n| !n.is_empty())
        {
            Some(n) => words("mcp-step", &[("name", client.name.as_str()), ("label", n)]),
            None => title.to_owned(),
        };
        let saved = {
            let label = label.clone();
            tokio::task::spawn_blocking(move || {
                Timeline::open(&root)
                    // A finished part: recorded even when the changes are
                    // already in the version made after a quiet minute.
                    .and_then(|mut t| t.checkpoint(CheckpointKind::Task, &label, None))
            })
            .await
        };
        match saved {
            Ok(Ok(entry)) => {
                // What it saves is no longer waiting for a version.
                self.state().mcp.with(|r| r.unsaved.remove(&client.id));
                let _ = self.app.emit(PROJECT_CHANGED_EVENT, ());
                match entry {
                    Some(e) => ToolOutput::text(format!("Saved version {}: {label}", e.id)),
                    None => ToolOutput::error("Could not save a version."),
                }
            }
            Ok(Err(e)) => ToolOutput::error(format!("Could not save a version: {e}")),
            Err(e) => ToolOutput::error(format!("Could not save a version: {e}")),
        }
    }

    async fn list_versions(&self) -> ToolOutput {
        if self.open_profile().await.1.is_none() {
            return ToolOutput::error(NO_PROJECT);
        }
        match crate::workspace::timeline(self.state()).await {
            Ok(rows) if rows.is_empty() => ToolOutput::text("The project has no versions yet."),
            Ok(rows) => {
                let lines: Vec<String> = rows
                    .iter()
                    .take(VERSIONS_SHOWN)
                    .map(|r| {
                        format!(
                            "- {} | {} | {} | {} files{}",
                            r.id,
                            r.kind,
                            r.label,
                            r.changed_files.len(),
                            if r.current { " | current" } else { "" }
                        )
                    })
                    .collect();
                ToolOutput::text(format!(
                    "id | kind | name | changed files (newest first)\n{}",
                    lines.join("\n")
                ))
            }
            Err(e) => ToolOutput::error(format!("Could not read the versions: {e}")),
        }
    }

    async fn restore(&self, client: &Client, step: &str, title: &str, args: &Value) -> ToolOutput {
        if let Err(refused) = self.idle_project().await {
            return refused;
        }
        let Some(id) = args["version"]
            .as_str()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        else {
            return ToolOutput::error("`version` is required: an id from list_versions.");
        };
        let rows = match crate::workspace::timeline(self.state()).await {
            Ok(rows) => rows,
            Err(e) => return ToolOutput::error(format!("Could not read the versions: {e}")),
        };
        let Some(row) = rows.iter().find(|r| r.id == id) else {
            return ToolOutput::error(format!(
                "There is no version `{id}`. Call list_versions for the ids."
            ));
        };
        if row.current {
            return ToolOutput::text("The project is at that version already.");
        }
        if !self
            .confirm(step, RESTORE, title, ConfirmKind::Delete, row.label.clone())
            .await
        {
            return ToolOutput::error(
                "The user did not want to go back to that version. Ask what they would \
                 prefer instead.",
            );
        }
        let state = self.state();
        state.mcp.with(|r| r.changing += 1);
        let _done = Done(Some(&state.mcp));
        if let Err(e) = crate::workspace::restore(self.state(), id.to_owned()).await {
            return ToolOutput::error(format!("Could not go back: {e}"));
        }
        state.mcp.with(|r| r.unsaved.remove(&client.id));
        let _ = self.app.emit(PROJECT_CHANGED_EVENT, ());
        ToolOutput::text(format!(
            "The project is back at version {id} ({}). The work before it is kept as a \
             version too (see list_versions).",
            row.label
        ))
    }

    async fn get_log(&self, args: &Value) -> ToolOutput {
        if self.open_profile().await.1.is_none() {
            return ToolOutput::error(NO_PROJECT);
        }
        let source: Option<LogSource> = match args.get("source").filter(|s| !s.is_null()) {
            Some(s) => match serde_json::from_value(s.clone()) {
                Ok(s) => Some(s),
                Err(_) => {
                    return ToolOutput::error(
                        "`source` is one of screens, backend, window, bundle, ai.",
                    );
                }
            },
            None => None,
        };
        let wanted = args["lines"]
            .as_u64()
            .map_or(LOG_LINES, |n| (n as usize).clamp(1, LOG_MAX));
        let lines = crate::workspace::get_log(self.state())
            .await
            .unwrap_or_default();
        let shown: Vec<String> = lines
            .iter()
            .filter(|l| l.progress.is_none() && source.is_none_or(|s| l.source == s))
            .map(|l| {
                let from = serde_json::to_value(l.source).unwrap_or_default();
                format!("[{}] {}", from.as_str().unwrap_or_default(), l.text)
            })
            .collect();
        if shown.is_empty() {
            return ToolOutput::text("The log has nothing there yet.");
        }
        ToolOutput::text(shown[shown.len().saturating_sub(wanted)..].join("\n"))
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
    /// As the open project's preview will: the tool list is the same then.
    fn can_capture(&self) -> bool {
        ak_capture::AVAILABLE
    }
    fn screenshot(&self, _: Option<String>) -> BoxFuture<'_, Result<ContentPart, String>> {
        Box::pin(async { Err("no preview".to_owned()) })
    }
}
