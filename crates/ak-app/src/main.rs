//! The AgentKnife window (the commands are in the library, src/lib.rs).

#![forbid(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use ak_app::{crash, state};

fn main() -> anyhow::Result<()> {
    crash::install(state::state_dir().join(crash::FOLDER));
    let state = state::AppState::load()?;
    ak_app::set_language(tauri::async_runtime::block_on(async {
        state.inner.lock().await.settings.language
    }));
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init());
    let app = ak_app::with_commands(builder, state).build(tauri::generate_context!())?;
    app.run(ak_app::on_event);
    Ok(())
}
