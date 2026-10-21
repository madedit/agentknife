//! The home screen's delete through the real command (mock runtime). Moving a
//! folder to the trash is not tested here: it would fill the real Recycle Bin.

#![allow(clippy::unwrap_used)]

use ak_app::state::AppState;
use ak_project::Registry;
use ak_types::i18n::Locale;
use ak_types::ipc::HomeState;
use serde_json::{Value, json};
use tauri::test::{INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context};
use tauri::webview::InvokeRequest;
use tauri::{WebviewWindow, WebviewWindowBuilder};

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

#[test]
fn removing_from_the_list_keeps_the_folder() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join("state");
    let projects = dir.path().join("Projects");
    let mut registry = Registry::load(&state_dir, &projects).unwrap();
    let keep = registry.create("留著", None, Locale::ZhTw).unwrap();
    let gone = registry.create("不要了", None, Locale::ZhTw).unwrap();
    let gone_id = gone.meta().id.clone();
    let gone_root = gone.root().to_path_buf();
    let keep_id = keep.meta().id.clone();
    let state = AppState::new(state_dir, registry, dir.path().join("toolchain")).unwrap();
    let app = ak_app::with_commands(mock_builder(), state)
        .build(mock_context(tauri::test::noop_assets()))
        .unwrap();
    let window = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();

    let home: HomeState = serde_json::from_value(
        call(
            &window,
            "delete_project",
            json!({ "id": gone_id, "folder": false }),
        )
        .unwrap(),
    )
    .unwrap();
    let ids: Vec<_> = home.projects.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, [keep_id.as_str()]);
    assert!(gone_root.is_dir(), "the folder stays on disk");
    // The list is saved: a fresh load agrees.
    let again = Registry::load(&dir.path().join("state"), &projects).unwrap();
    assert_eq!(again.recent().len(), 1);

    let unknown = call(
        &window,
        "delete_project",
        json!({ "id": "nope", "folder": false }),
    );
    assert!(unknown.is_err());
}
