//! The home screen's commands, and opening a project (preview, conversation,
//! interruption, greeting).

use std::sync::Arc;
use std::time::Duration;

use ak_preview::{Preview, PreviewSession, profiles};
use ak_project::{Project, RecentProject, Timeline};
use ak_types::ipc::{HomeState, NewProject, OpenedProject, PREVIEW_EVENT, ProjectRow};
use ak_types::stack::StackProfile;
use tauri::{AppHandle, Emitter, Runtime, State};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::Mutex;

use crate::state::{AppState, OpenProject};
use crate::{Res, fail, ui_locale};

fn row(r: &RecentProject) -> ProjectRow {
    ProjectRow {
        id: r.entry.id.clone(),
        name: r.entry.name.clone(),
        path: r.entry.path.display().to_string(),
        last_opened_at: r.entry.last_opened_at,
        exists: r.exists,
    }
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
    })
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
    open(app, &state, project).await
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
    open(app, &state, project).await
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
    open(app, &state, project).await.map(Some)
}

/// Closes what is open: stops the turn, the preview and the app window.
pub async fn close(open: Option<OpenProject>) {
    let Some(mut o) = open else { return };
    if let Some(c) = &o.cancel {
        c.cancel();
    }
    if let Some(child) = o.app_window.as_mut() {
        let _ = child.start_kill();
    }
    o.preview.stop().await;
}

async fn open<R: Runtime>(
    app: AppHandle<R>,
    state: &State<'_, AppState>,
    project: Project,
) -> Res<OpenedProject> {
    let root = project.root().to_path_buf();
    let id = project.meta().id.clone();
    let profile = profile_of(&project);

    let previous = state.inner.lock().await.open.take();
    close(previous).await;

    let preview = Preview::new(&root, profile.preview(), state.tools.clone()).map_err(fail)?;
    let session = Arc::new(PreviewSession::new(preview, None));
    session.start().await.map_err(fail)?;
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
        });
    }
    // The project notes for the AI, and a greeting in a brand-new chat (after
    // the window has shown the history, so its events land after it).
    let greet = empty && interrupted.is_none();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(400)).await;
        crate::turn::on_open(app, greet).await;
    });
    Ok(OpenedProject {
        project: row,
        interrupted,
        history: entries,
    })
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
