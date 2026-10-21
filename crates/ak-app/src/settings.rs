//! The settings page's commands.

use std::path::Path;

use ak_types::ipc::CrashReports;
use ak_types::settings::AppSettings;
use tauri::{AppHandle, Runtime, State};
use tauri_plugin_dialog::DialogExt;

use crate::state::{AppState, SettingsFile};
use crate::{Res, fail, set_language};

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Res<AppSettings> {
    Ok(state.inner.lock().await.settings.clone())
}

/// Saves the page's values (clamped). The AI connection is kept as it is:
/// only the connect wizard changes it, with its key.
#[tauri::command]
pub async fn save_settings(state: State<'_, AppState>, request: AppSettings) -> Res<()> {
    let mut inner = state.inner.lock().await;
    let ai = inner.settings.ai.clone();
    let mut settings = AppSettings { ai, ..request }.clamped();
    settings.read_folders.retain(|f| Path::new(f).is_absolute());
    inner.settings = settings;
    inner.settings.save(&state.state_dir).map_err(fail)?;
    set_language(inner.settings.language);
    Ok(())
}

/// A folder the user chooses (for the readable folders).
#[tauri::command]
pub async fn pick_folder<R: Runtime>(app: AppHandle<R>) -> Res<Option<String>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_folder(move |picked| {
        let _ = tx.send(picked);
    });
    let Some(picked) = rx.await.ok().flatten() else {
        return Ok(None);
    };
    let path = picked.into_path().map_err(fail)?;
    Ok(Some(path.display().to_string()))
}

#[tauri::command]
pub async fn crash_reports(state: State<'_, AppState>) -> Res<CrashReports> {
    Ok(crate::crash::list(
        &state.state_dir.join(crate::crash::FOLDER),
    ))
}
