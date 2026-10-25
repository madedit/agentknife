//! One project's running preview as the rest of AgentKnife uses it: build state,
//! the control channel, the user's last pick, screenshots.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ak_types::preview::{
    AppState, ControlAction, DomSummary, PAGE_CLOSED, PAGE_NOT_OPEN, PAGE_RELOADED_STUCK,
    PAGE_TIMEOUT, Picked, PreviewStatus, ScreenshotArea, ServerState,
};
use futures_util::future::BoxFuture;
use tokio::sync::watch;

use crate::Preview;
use crate::control::Controller;

/// How long a control action may take (a click waits for the page to settle).
const ACTION_TIMEOUT: Duration = Duration::from_secs(15);
/// How long to wait for the page to come back (it reloads after each rebuild).
const RELOAD_GRACE: Duration = Duration::from_secs(10);

/// A captured image of the preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub media_type: String,
    pub data_base64: String,
}

/// Takes the preview's pixels. The page cannot photograph itself; the window
/// that shows it can (ak-app, with the platform web view's capture).
pub trait Capturer: Send + Sync {
    fn capture<'a>(&'a self, area: &'a ScreenshotArea) -> BoxFuture<'a, Result<Image, String>>;
}

pub struct PreviewSession {
    preview: tokio::sync::Mutex<Preview>,
    status: watch::Receiver<PreviewStatus>,
    controller: Controller,
    selection: Mutex<Option<Picked>>,
    capturer: Option<Arc<dyn Capturer>>,
    crashes: crate::log::CrashLog,
    /// Loads the page again in whoever shows it (the app's preview pane).
    reloader: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

/// How often the backend code on disk is checked.
pub const WATCH_EVERY: Duration = Duration::from_millis(1_500);

fn busy(s: &ServerState) -> bool {
    matches!(s, ServerState::Starting | ServerState::Building)
}

impl PreviewSession {
    pub fn new(preview: Preview, capturer: Option<Arc<dyn Capturer>>) -> Self {
        Self {
            status: preview.subscribe(),
            controller: Controller::new(preview.controller_url()),
            crashes: preview.crashes(),
            reloader: Mutex::new(None),
            preview: tokio::sync::Mutex::new(preview),
            selection: Mutex::new(None),
            capturer,
        }
    }

    pub async fn start(&self) -> std::io::Result<()> {
        self.preview.lock().await.start()
    }

    pub async fn stop(&self) {
        self.preview.lock().await.stop().await;
    }

    /// Checks the backend code on disk every [`WATCH_EVERY`] while the
    /// session lives, and restarts the backend when it changed without a
    /// [`PreviewSession::files_changed`].
    pub fn watch_backend(self: &Arc<Self>) {
        let session = Arc::downgrade(self);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(WATCH_EVERY).await;
                let Some(s) = session.upgrade() else { return };
                let _ = s.preview.lock().await.backend_changed_on_disk().await;
            }
        });
    }

    pub fn status(&self) -> PreviewStatus {
        self.status.borrow().clone()
    }

    /// How to load the page again when it stops answering (e.g. an endless loop
    /// in the screens: a rebuild cannot reach it either).
    pub fn set_reloader(&self, reload: impl Fn() + Send + Sync + 'static) {
        if let Ok(mut r) = self.reloader.lock() {
            *r = Some(Arc::new(reload));
        }
    }

    /// The backend's crash reports since it last started, oldest first.
    pub fn backend_crashes(&self) -> Vec<String> {
        self.crashes.reports()
    }

    pub fn subscribe(&self) -> watch::Receiver<PreviewStatus> {
        self.status.clone()
    }

    pub async fn page_url(&self) -> String {
        self.preview.lock().await.page_url()
    }

    /// Waits until nothing is building (or `timeout`), then returns the state.
    pub async fn settled(&self, timeout: Duration) -> PreviewStatus {
        let mut rx = self.status.clone();
        let wait = async {
            loop {
                {
                    let s = rx.borrow_and_update();
                    if !busy(&s.frontend) && !busy(&s.backend) {
                        return;
                    }
                }
                if rx.changed().await.is_err() {
                    return;
                }
            }
        };
        let _ = tokio::time::timeout(timeout, wait).await;
        self.status()
    }

    /// See `Preview::files_changed`.
    pub async fn files_changed(&self, paths: &[String]) -> std::io::Result<bool> {
        self.preview.lock().await.files_changed(paths).await
    }

    /// Runs an action in the page. The page reloads after every rebuild; while it
    /// is away, the action is retried for a while: always if it never reached a
    /// page, and only if it is read-only when the page left before answering.
    pub async fn act(&self, action: &ControlAction) -> Result<serde_json::Value, String> {
        let deadline = tokio::time::Instant::now() + RELOAD_GRACE;
        loop {
            match self.controller.act(action, ACTION_TIMEOUT).await {
                Err(e)
                    if tokio::time::Instant::now() < deadline
                        && (e == PAGE_NOT_OPEN || (e == PAGE_CLOSED && action.is_read_only())) =>
                {
                    tokio::time::sleep(Duration::from_millis(300)).await;
                }
                Err(e) if e == PAGE_TIMEOUT => {
                    let reload = self.reloader.lock().ok().and_then(|r| r.clone());
                    return Err(match reload {
                        Some(reload) => {
                            reload();
                            PAGE_RELOADED_STUCK.into()
                        }
                        None => e,
                    });
                }
                other => return other,
            }
        }
    }

    pub async fn dom_summary(&self) -> Result<DomSummary, String> {
        let v = self.act(&ControlAction::DomSummary).await?;
        serde_json::from_value(v).map_err(|e| format!("unexpected page summary: {e}"))
    }

    pub async fn app_state(&self) -> Result<AppState, String> {
        let v = self.act(&ControlAction::AppState).await?;
        serde_json::from_value(v).map_err(|e| format!("unexpected app state: {e}"))
    }

    /// The user's latest pick in the inspector (the app forwards inspector
    /// `select` events here).
    pub fn set_selection(&self, picked: Option<Picked>) {
        if let Ok(mut s) = self.selection.lock() {
            *s = picked;
        }
    }

    pub fn selection(&self) -> Option<Picked> {
        self.selection.lock().ok().and_then(|s| s.clone())
    }

    pub fn can_capture(&self) -> bool {
        self.capturer.is_some()
    }

    /// Scrolls `ak_id` (or nothing) into view, lets the page paint, captures.
    pub async fn screenshot(
        &self,
        ak_id: Option<String>,
    ) -> Result<(Image, ScreenshotArea), String> {
        let capturer = self
            .capturer
            .as_ref()
            .ok_or("Screenshots are not available here.")?;
        let v = self
            .act(&ControlAction::PrepareScreenshot { ak_id })
            .await?;
        let area: ScreenshotArea =
            serde_json::from_value(v).map_err(|e| format!("unexpected screenshot area: {e}"))?;
        let image = capturer.capture(&area).await?;
        Ok((image, area))
    }
}
