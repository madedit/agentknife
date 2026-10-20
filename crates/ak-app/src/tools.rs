//! The tools check (ak-doctor), packaging (ak-bundle), and opening things
//! outside the window.

use std::path::Path;
use std::time::Duration;

use ak_bundle::guide::{install_guide, reveal as reveal_path};
use ak_bundle::{BundleError, BundleEvent, Bundler};
use ak_doctor::fetch::HttpFetch;
use ak_doctor::probe::SystemProbe;
use ak_doctor::{Installer, Platform, assets};
use ak_types::doctor::SystemAction;
use ak_types::ipc::{
    BUNDLE_EVENT, BundleFailure, BundleProgress, BundleResult, DOCTOR_EVENT, DoctorCheck,
};
use ak_types::stack::HostOs;
use tauri::{AppHandle, Emitter, Runtime, State};
use tauri_plugin_opener::OpenerExt;
use tokio_util::sync::CancellationToken;

use crate::state::AppState;
use crate::{Res, fail};

/// Installers (winget, cargo install) may take hours; checks are short.
const RUNNER_TIMEOUT: Duration = Duration::from_secs(2 * 60 * 60);

async fn check_now(state: &AppState) -> DoctorCheck {
    let probe = SystemProbe::default();
    let platform = Platform::current();
    let report = ak_doctor::check(&state.toolchain, &state.pinned, platform, &probe).await;
    let guides = ak_doctor::system::guides(&report, platform, &probe).await;
    DoctorCheck { report, guides }
}

#[tauri::command]
pub async fn doctor_check(state: State<'_, AppState>) -> Res<DoctorCheck> {
    Ok(check_now(&state).await)
}

#[tauri::command]
pub async fn doctor_install<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> Res<DoctorCheck> {
    let fetch = HttpFetch::new().map_err(fail)?;
    let probe = SystemProbe::default();
    let runner = SystemProbe {
        timeout: RUNNER_TIMEOUT,
    };
    let installer = Installer {
        toolchain: &state.toolchain,
        pinned: &state.pinned,
        platform: Platform::current(),
        probe: &probe,
        runner: &runner,
        fetch: &fetch,
        target: assets::host_target(),
    };
    let report = installer
        .install(&|p| {
            let _ = app.emit(DOCTOR_EVENT, p);
        })
        .await;
    let guides = ak_doctor::system::guides(&report, Platform::current(), &probe).await;
    Ok(DoctorCheck { report, guides })
}

#[tauri::command]
pub async fn doctor_action(request: SystemAction) -> Res<()> {
    let probe = SystemProbe::default();
    let runner = SystemProbe {
        timeout: RUNNER_TIMEOUT,
    };
    ak_doctor::system::launch(&request, &probe, &runner)
        .await
        .map_err(fail)
}

/// How the window names a failure (`bundle-error-<kind>`).
pub fn failure(e: BundleError) -> BundleFailure {
    let (kind, detail) = match e {
        BundleError::UnsupportedHost => ("unsupported", String::new()),
        BundleError::AppInfo { .. } => ("app-info", e.to_string()),
        BundleError::BuildFailed { output } => ("build-failed", output),
        BundleError::PreviewCode { .. } => ("preview-code", e.to_string()),
        BundleError::Cancelled => ("cancelled", String::new()),
        other => ("other", other.to_string()),
    };
    BundleFailure {
        kind: kind.into(),
        detail,
    }
}

fn os_key(os: HostOs) -> &'static str {
    match os {
        HostOs::Windows => "windows",
        HostOs::MacOs => "macos",
        HostOs::Linux => "linux",
    }
}

#[tauri::command]
pub async fn bundle<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> Result<BundleResult, serde_json::Value> {
    let as_value = |f: BundleFailure| serde_json::to_value(f).unwrap_or(serde_json::Value::Null);
    let (root, profile) = {
        let inner = state.inner.lock().await;
        let open = inner.open.as_ref().ok_or_else(|| {
            as_value(BundleFailure {
                kind: "other".into(),
                detail: "no project is open".into(),
            })
        })?;
        let profile = open
            .project
            .lock()
            .map(|p| crate::home::profile_of(&p))
            .map_err(|_| as_value(failure(BundleError::Cancelled)))?;
        (open.root.clone(), profile)
    };
    let os = HostOs::current().ok_or_else(|| as_value(failure(BundleError::UnsupportedHost)))?;
    let bundler = Bundler::new(&root, profile.bundle(), state.tools.clone());
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let forward = {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(e) = rx.recv().await {
                let p = match e {
                    BundleEvent::Stage(s) => BundleProgress::Stage {
                        message: s.message_id().into(),
                    },
                    BundleEvent::Output(line) => BundleProgress::Output { line },
                };
                let _ = app.emit(BUNDLE_EVENT, p);
            }
        })
    };
    let result = bundler.build(&tx, &CancellationToken::new()).await;
    drop(tx);
    let _ = forward.await;
    let b = result.map_err(|e| as_value(failure(e)))?;
    Ok(BundleResult {
        file: b
            .path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default(),
        path: b.path.display().to_string(),
        app: b.app,
        os: os_key(os).into(),
        steps: install_guide(os, b.output),
    })
}

#[tauri::command]
pub fn reveal(request: String) -> Res<()> {
    reveal_path(Path::new(&request)).map_err(fail)
}

/// Opens an https page in the user's browser.
#[tauri::command]
pub fn open_url<R: Runtime>(app: AppHandle<R>, request: String) -> Res<()> {
    if !request.starts_with("https://") {
        return Err(fail("only https pages open from AgentKnife"));
    }
    app.opener().open_url(request, None::<&str>).map_err(fail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_get_their_plain_words() {
        assert_eq!(failure(BundleError::UnsupportedHost).kind, "unsupported");
        let f = failure(BundleError::BuildFailed {
            output: "error[E0308]".into(),
        });
        assert_eq!(
            (f.kind.as_str(), f.detail.as_str()),
            ("build-failed", "error[E0308]")
        );
        assert_eq!(failure(BundleError::Cancelled).kind, "cancelled");
        assert_eq!(failure(BundleError::Missing("x".into())).kind, "other");
    }
}
