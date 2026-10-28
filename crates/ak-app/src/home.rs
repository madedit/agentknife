//! The home screen's commands, and opening a project (preview, conversation,
//! interruption, greeting).

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use ak_preview::log::LogBook;
use ak_preview::{Preview, PreviewSession, profiles};
use ak_project::{Project, RecentProject, Timeline};
use ak_types::ipc::{
    DeleteProject, HomeState, KitChanges, LOG_EVENT, NewProject, OpenedProject, PREVIEW_EVENT,
    ProjectRow,
};
use ak_types::log::LogSource;
use ak_types::stack::StackProfile;
use tauri::{AppHandle, Emitter, Runtime, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use tokio::sync::Mutex;

use crate::state::{AppState, OpenProject};
use crate::{Res, fail, ui_locale};

/// Moving a folder to the trash: tries after the first, and the wait between.
const TRASH_TRIES: u32 = 5;
const TRASH_WAIT: Duration = Duration::from_millis(400);
/// New log lines are sent to the window at most this often.
const LOG_BATCH: Duration = Duration::from_millis(200);

fn row(r: &RecentProject) -> ProjectRow {
    ProjectRow {
        id: r.entry.id.clone(),
        name: r.entry.name.clone(),
        path: r.entry.path.display().to_string(),
        last_opened_at: r.entry.last_opened_at,
        exists: r.exists,
    }
}

/// The open project's stack.
pub fn profile_of_root(open: &OpenProject) -> &'static dyn StackProfile {
    open.project
        .lock()
        .map(|p| profile_of(&p))
        .unwrap_or_else(|_| profiles::default_profile())
}

/// The project's stack (the default one for projects made before stacks).
pub fn profile_of(project: &Project) -> &'static dyn StackProfile {
    project
        .meta()
        .stack
        .as_deref()
        .and_then(profiles::by_id)
        .unwrap_or_else(profiles::default_profile)
}

#[tauri::command]
pub async fn home_state(state: State<'_, AppState>) -> Res<HomeState> {
    let inner = state.inner.lock().await;
    Ok(HomeState {
        projects: inner.registry.recent().iter().map(row).collect(),
        ai: inner.settings.ai.clone(),
        projects_folder: inner.registry.default_parent().display().to_string(),
        external: inner.settings.mcp_enabled,
    })
}

/// Takes a project off the list; with `folder`, its folder goes to the trash.
/// An open project is closed first: its preview keeps files in use.
#[tauri::command]
pub async fn delete_project(state: State<'_, AppState>, request: DeleteProject) -> Res<HomeState> {
    let (open, path) = {
        let mut inner = state.inner.lock().await;
        let path = inner
            .registry
            .recent()
            .iter()
            .find(|r| r.entry.id == request.id)
            .map(|r| r.entry.path.clone())
            .ok_or_else(|| fail(format!("no project {}", request.id)))?;
        let is_open = inner.open.as_ref().is_some_and(|o| o.root == path);
        (if is_open { inner.open.take() } else { None }, path)
    };
    close(open).await;
    if request.folder && path.is_dir() {
        trash_folder(&path).await.map_err(fail)?;
    }
    state
        .inner
        .lock()
        .await
        .registry
        .forget(&request.id)
        .map_err(fail)?;
    home_state(state).await
}

/// Opens a project's folder in the file manager: a listed project by id, or
/// the open one. Only project folders (the window names none itself).
#[tauri::command]
pub async fn open_in_files<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    request: Option<String>,
) -> Res<()> {
    let folder = {
        let inner = state.inner.lock().await;
        match &request {
            Some(id) => inner
                .registry
                .recent()
                .into_iter()
                .find(|r| &r.entry.id == id)
                .map(|r| r.entry.path),
            None => inner.open.as_ref().map(|o| o.root.clone()),
        }
    }
    .ok_or_else(|| fail("no such project"))?;
    if !folder.is_dir() {
        return Err(fail("the project folder is missing"));
    }
    app.opener()
        .open_path(folder.display().to_string(), None::<&str>)
        .map_err(fail)
}

/// Moves a folder to the system's trash. Windows releases the files of just
/// stopped programs a moment later, so it tries a few times.
async fn trash_folder(path: &Path) -> Result<(), trash::Error> {
    let mut tries = 0;
    loop {
        match trash::delete(path) {
            Err(_) if tries < TRASH_TRIES => {
                tries += 1;
                tokio::time::sleep(TRASH_WAIT).await;
            }
            done => return done,
        }
    }
}

#[tauri::command]
pub async fn create_project<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    request: NewProject,
) -> Res<OpenedProject> {
    let mut project = {
        let mut inner = state.inner.lock().await;
        inner
            .registry
            .create(&request.name, None, ui_locale())
            .map_err(fail)?
    };
    project
        .scaffold(profiles::default_profile())
        .map_err(fail)?;
    open(app, &state, project, true).await
}

/// Makes and opens a project for another AI tool (MCP), without a greeting
/// from AgentKnife's own AI.
pub async fn create_project_quietly<R: Runtime>(
    app: AppHandle<R>,
    state: &State<'_, AppState>,
    name: &str,
) -> Res<OpenedProject> {
    let mut project = {
        let mut inner = state.inner.lock().await;
        inner
            .registry
            .create(name, None, ui_locale())
            .map_err(fail)?
    };
    project
        .scaffold(profiles::default_profile())
        .map_err(fail)?;
    open(app, state, project, false).await
}

#[tauri::command]
pub async fn open_project<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    request: String,
) -> Res<OpenedProject> {
    let project = {
        let mut inner = state.inner.lock().await;
        let path = inner
            .registry
            .recent()
            .into_iter()
            .find(|r| r.entry.id == request)
            .map(|r| r.entry.path)
            .ok_or_else(|| fail(format!("no project {request}")))?;
        inner.registry.open(&path, ui_locale()).map_err(fail)?
    };
    open(app, &state, project, true).await
}

/// Opens a listed project for another AI tool (MCP): no greeting from
/// AgentKnife's own AI, which would only get in its way.
pub async fn open_project_quietly<R: Runtime>(
    app: AppHandle<R>,
    state: &State<'_, AppState>,
    id: String,
) -> Res<OpenedProject> {
    let project = {
        let mut inner = state.inner.lock().await;
        let path = inner
            .registry
            .recent()
            .into_iter()
            .find(|r| r.entry.id == id)
            .map(|r| r.entry.path)
            .ok_or_else(|| fail(format!("no project {id}")))?;
        inner.registry.open(&path, ui_locale()).map_err(fail)?
    };
    open(app, state, project, false).await
}

#[tauri::command]
pub async fn open_folder<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> Res<Option<OpenedProject>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_folder(move |picked| {
        let _ = tx.send(picked);
    });
    let Some(picked) = rx.await.ok().flatten() else {
        return Ok(None);
    };
    let path = picked.into_path().map_err(fail)?;
    let mut project = {
        let mut inner = state.inner.lock().await;
        inner.registry.open(&path, ui_locale()).map_err(fail)?
    };
    // A plain folder gets the starter app; an existing app is left as it is.
    if !path.join("Cargo.toml").exists() {
        project
            .scaffold(profiles::default_profile())
            .map_err(fail)?;
    }
    open(app, &state, project, true).await.map(Some)
}

/// Closes what is open: stops the turn, the preview and the app window.
pub async fn close(open: Option<OpenProject>) {
    let Some(o) = open else { return };
    if let Some(c) = &o.cancel {
        c.cancel();
    }
    // The whole tree: `cargo tauri dev` leaves trunk and the app behind.
    if let Some(pid) = o.app_window.as_ref().and_then(|c| c.id()) {
        ak_preview::process::kill_tree(pid).await;
    }
    o.preview.stop().await;
}

async fn open<R: Runtime>(
    app: AppHandle<R>,
    state: &State<'_, AppState>,
    project: Project,
    may_greet: bool,
) -> Res<OpenedProject> {
    let root = project.root().to_path_buf();
    let id = project.meta().id.clone();
    let profile = profile_of(&project);

    let previous = state.inner.lock().await.open.take();
    close(previous).await;

    let preview = Preview::new(&root, profile.preview(), state.tools.clone()).map_err(fail)?;
    let log = preview.log();
    forward_log(app.clone(), log.clone());
    let session = Arc::new(PreviewSession::new(
        preview,
        crate::capture::capturer(&app, state),
    ));
    // A page stuck in a loop cannot reload itself; the preview pane can.
    let reload_app = app.clone();
    session.set_reloader(move || {
        let _ = reload_app.emit(ak_types::ipc::PREVIEW_RELOAD_EVENT, ());
    });
    // On a computer without the tools yet the preview cannot start; the
    // project opens anyway, the window offers to install them, and
    // `doctor_install` starts the preview once they are there.
    if let Err(e) = session.start().await {
        let note = if e.kind() == std::io::ErrorKind::NotFound {
            "> the preview starts once AgentKnife's tools are installed".to_owned()
        } else {
            format!("> the preview did not start: {e}")
        };
        log.push(LogSource::Screens, &note);
    }
    // Backend code changed by another AI tool's own file tools, or by the user.
    session.watch_backend();
    forward_status(app.clone(), session.clone());

    let history = project.load_conversation().map_err(fail)?.conversation;
    let interrupted = Timeline::open(&root)
        .and_then(|t| t.interruption())
        .ok()
        .flatten()
        .and_then(|i| i.turn)
        .map(|t| t.request);
    let entries = history.entries().to_vec();
    let empty = history.messages().is_empty();
    // An older kit (or one edited before the AI's tools refused it): offered.
    let kit_outdated = project.kit_changes(profile).unwrap_or_default();

    let row = {
        let inner = state.inner.lock().await;
        inner
            .registry
            .recent()
            .iter()
            .find(|r| r.entry.id == id)
            .map(row)
            .ok_or_else(|| fail("the project is not in the list"))?
    };
    {
        let mut inner = state.inner.lock().await;
        inner.open = Some(OpenProject {
            root: root.clone(),
            project: Arc::new(std::sync::Mutex::new(project)),
            preview: session,
            conversation: Arc::new(Mutex::new(history)),
            cancel: None,
            before_turn: None,
            app_window: None,
            log,
            added_files: Vec::new(),
            unannounced: Vec::new(),
            renamed_files: Vec::new(),
            unannounced_renames: Vec::new(),
            kit_outdated: kit_outdated.clone(),
            kit_note: None,
            unannounced_kit: None,
        });
    }
    // The project notes for the AI, and a greeting in a brand-new chat (after
    // the window has shown the history, so its events land after it).
    let greet = may_greet && empty && interrupted.is_none();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(400)).await;
        crate::turn::on_open(app, greet).await;
    });
    Ok(OpenedProject {
        project: row,
        interrupted,
        history: entries,
        kit_update: (!kit_outdated.is_empty()).then_some(KitChanges {
            files: kit_outdated,
        }),
    })
}

/// Sends new log lines to the window, a batch at a time.
fn forward_log<R: Runtime>(app: AppHandle<R>, log: LogBook) {
    let mut rx = log.subscribe();
    tauri::async_runtime::spawn(async move {
        while let Some(first) = rx.recv().await {
            tokio::time::sleep(LOG_BATCH).await;
            let mut batch = vec![first];
            while let Ok(line) = rx.try_recv() {
                batch.push(line);
            }
            if app.emit(LOG_EVENT, batch).is_err() {
                break;
            }
        }
    });
}

/// Sends every status change of the preview to the window.
fn forward_status<R: Runtime>(app: AppHandle<R>, session: Arc<PreviewSession>) {
    let mut rx = session.subscribe();
    tauri::async_runtime::spawn(async move {
        let _ = app.emit(PREVIEW_EVENT, rx.borrow().clone());
        while rx.changed().await.is_ok() {
            let status = rx.borrow().clone();
            if app.emit(PREVIEW_EVENT, status).is_err() {
                break;
            }
        }
    });
}
