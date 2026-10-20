//! The connect wizard's commands: test a connection, check a model, save it.

use std::sync::Arc;

use ak_providers::connect::{check_model as check, list_models as list, provider_for};
use ak_providers::credentials::{ApiKey, CredentialStore, Keychain, account};
use ak_providers::{Provider, ReqwestTransport};
use ak_types::ipc::{ConnectError, ConnectRequest, ModelCheck, ModelChoice};
use ak_types::settings::AiConnection;
use tauri::State;

use crate::state::AppState;

fn to_value(e: ConnectError) -> serde_json::Value {
    serde_json::to_value(e).unwrap_or(serde_json::Value::Null)
}

/// The key the wizard typed, else the one in the keychain.
fn key(conn: &AiConnection, typed: Option<&str>) -> Option<ApiKey> {
    typed
        .and_then(ApiKey::new)
        .or_else(|| Keychain.get(&account(conn)).ok().flatten())
}

/// The provider for a connection (the key from the keychain when not given).
pub fn provider(
    conn: &AiConnection,
    typed: Option<&str>,
) -> Result<Arc<dyn Provider>, ConnectError> {
    let transport = ReqwestTransport::new().map_err(|e| ConnectError::Other {
        detail: e.to_string(),
    })?;
    provider_for(conn, key(conn, typed), Arc::new(transport))
}

#[tauri::command]
pub async fn list_models(request: ConnectRequest) -> Result<Vec<ModelChoice>, serde_json::Value> {
    let p = provider(&request.connection, request.api_key.as_deref()).map_err(to_value)?;
    list(p.as_ref()).await.map_err(to_value)
}

#[tauri::command]
pub async fn check_model(request: ConnectRequest) -> Result<ModelCheck, serde_json::Value> {
    let p = provider(&request.connection, request.api_key.as_deref()).map_err(to_value)?;
    check(p.as_ref(), &request.connection.model)
        .await
        .map_err(to_value)
}

#[tauri::command]
pub async fn save_connection(
    state: State<'_, AppState>,
    request: ConnectRequest,
) -> Result<(), serde_json::Value> {
    if let Some(k) = request.api_key.as_deref().and_then(ApiKey::new) {
        Keychain
            .set(&account(&request.connection), &k)
            .map_err(|e| {
                to_value(ConnectError::Other {
                    detail: e.to_string(),
                })
            })?;
    }
    let mut inner = state.inner.lock().await;
    inner.settings.ai = Some(request.connection);
    use crate::state::SettingsFile;
    inner.settings.save(&state.state_dir).map_err(|e| {
        to_value(ConnectError::Other {
            detail: e.to_string(),
        })
    })
}
