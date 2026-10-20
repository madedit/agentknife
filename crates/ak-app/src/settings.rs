//! The settings page's commands.

use ak_types::ipc::CrashReports;
use ak_types::settings::AppSettings;
use tauri::State;

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
    inner.settings = AppSettings { ai, ..request }.clamped();
    inner.settings.save(&state.state_dir).map_err(fail)?;
    set_language(inner.settings.language);
    Ok(())
}

#[tauri::command]
pub async fn crash_reports(state: State<'_, AppState>) -> Res<CrashReports> {
    Ok(crate::crash::list(
        &state.state_dir.join(crate::crash::FOLDER),
    ))
}
