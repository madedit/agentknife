//! The live preview of one project: the screens' dev server and the app's
//! backend, started from the stack's [`PreviewPlan`], with their state published
//! as a [`PreviewStatus`].

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ak_types::preview::{PreviewStatus, ServerState};
use ak_types::stack::{PreviewPlan, ReadySignal};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::log::{BuildTracker, Tail};
use crate::process::{
    Tools, encode_query_value, free_port, kill_tree, kill_tree_now, random_token,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Frontend,
    Backend,
}

struct Running {
    pid: Option<u32>,
    /// Set before an intended stop, so the exit is not reported as a failure.
    stopping: Arc<AtomicBool>,
    task: JoinHandle<()>,
}

pub struct Preview {
    root: PathBuf,
    plan: PreviewPlan,
    tools: Tools,
    port: u16,
    bridge_port: u16,
    token: String,
    status: watch::Sender<PreviewStatus>,
    frontend: Option<Running>,
    backend: Option<Running>,
}

impl Preview {
    /// Prepares the preview of `root`: picks ports and a bridge token. Nothing
    /// runs until [`Preview::start`].
    pub fn new(root: &Path, plan: PreviewPlan, tools: Tools) -> std::io::Result<Preview> {
        let (status, _) = watch::channel(PreviewStatus::default());
        Ok(Preview {
            root: root.to_path_buf(),
            plan,
            tools,
            port: free_port()?,
            // Kept across backend restarts: the page reconnects to the same address.
            bridge_port: free_port()?,
            token: random_token()?,
            status,
            frontend: None,
            backend: None,
        })
    }

    /// Follow the state (the UI and the agent's build checks watch this).
    pub fn subscribe(&self) -> watch::Receiver<PreviewStatus> {
        self.status.subscribe()
    }

    pub fn status(&self) -> PreviewStatus {
        self.status.borrow().clone()
    }

    fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn bridge_ws(&self) -> String {
        format!("ws://127.0.0.1:{}/?token={}", self.bridge_port, self.token)
    }

    /// The page to show: the dev server, told where the bridge is.
    pub fn page_url(&self) -> String {
        format!(
            "{}/?{}={}",
            self.origin(),
            self.plan.bridge_url_param,
            encode_query_value(&self.bridge_ws())
        )
    }

    /// For the agent's control connection (P1-PREV-002).
    pub fn controller_url(&self) -> String {
        format!("{}&role=controller", self.bridge_ws())
    }

    fn values(&self) -> Vec<(&'static str, String)> {
        vec![
            ("port", self.port.to_string()),
            ("bridge_port", self.bridge_port.to_string()),
            ("bridge_token", self.token.clone()),
            ("bridge_origin", self.origin()),
        ]
    }

    /// Starts the dev server and the backend (if the stack has one).
    pub fn start(&mut self) -> std::io::Result<()> {
        if self.frontend.is_none() {
            self.frontend = Some(self.spawn(Side::Frontend)?);
        }
        if self.backend.is_none() && self.plan.backend.is_some() {
            self.backend = Some(self.spawn(Side::Backend)?);
        }
        Ok(())
    }

    /// Stops everything.
    pub async fn stop(&mut self) {
        for side in [Side::Frontend, Side::Backend] {
            self.stop_side(side).await;
        }
    }

    pub async fn restart(&mut self) -> std::io::Result<()> {
        self.stop().await;
        self.start()
    }

    /// Restarts the backend (e.g. after its code changed).
    pub async fn restart_backend(&mut self) -> std::io::Result<()> {
        if self.plan.backend.is_none() {
            return Ok(());
        }
        self.stop_side(Side::Backend).await;
        self.backend = Some(self.spawn(Side::Backend)?);
        Ok(())
    }

    /// Tells the preview which project files changed (relative paths). Restarts
    /// the backend when one of them is backend code; the dev server rebuilds the
    /// screens by itself. Returns whether the backend was restarted.
    pub async fn files_changed(&mut self, paths: &[String]) -> std::io::Result<bool> {
        let backend_code = paths.iter().any(|p| {
            let p = p.replace('\\', "/");
            self.plan
                .backend_watch
                .iter()
                .any(|w| p == *w || p.starts_with(&format!("{w}/")))
        });
        if backend_code && self.backend.is_some() {
            self.restart_backend().await?;
            return Ok(true);
        }
        Ok(false)
    }

    async fn stop_side(&mut self, side: Side) {
        let slot = match side {
            Side::Frontend => self.frontend.take(),
            Side::Backend => self.backend.take(),
        };
        if let Some(r) = slot {
            r.stopping.store(true, Ordering::SeqCst);
            if let Some(pid) = r.pid {
                kill_tree(pid).await;
            }
            let _ = r.task.await;
        }
        set(&self.status, side, ServerState::Off, None);
    }

    fn spawn(&self, side: Side) -> std::io::Result<Running> {
        let spec = match side {
            Side::Frontend => &self.plan.frontend,
            Side::Backend => self
                .plan
                .backend
                .as_ref()
                .ok_or_else(|| std::io::Error::other("the stack has no backend"))?,
        };
        let values = self.values();
        let refs: Vec<(&str, &str)> = values.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let mut child = self.tools.command(spec, &self.root, &refs).spawn()?;
        let pid = child.id();
        set(&self.status, side, ServerState::Starting, None);

        // stdout and stderr, line by line, into one stream.
        let (tx, rx) = mpsc::unbounded_channel::<String>();
        if let Some(out) = child.stdout.take() {
            pipe(out, tx.clone());
        }
        if let Some(err) = child.stderr.take() {
            pipe(err, tx);
        }
        let stopping = Arc::new(AtomicBool::new(false));
        let watcher = Watcher {
            side,
            status: self.status.clone(),
            plan: self.plan.clone(),
            page_url: self.page_url(),
            stopping: stopping.clone(),
        };
        let task = tokio::spawn(watcher.run(child, rx));
        Ok(Running {
            pid,
            stopping,
            task,
        })
    }
}

impl Drop for Preview {
    fn drop(&mut self) {
        for r in [self.frontend.take(), self.backend.take()]
            .into_iter()
            .flatten()
        {
            r.stopping.store(true, Ordering::SeqCst);
            if let Some(pid) = r.pid {
                kill_tree_now(pid);
            }
            r.task.abort();
        }
    }
}

fn pipe<R>(reader: R, tx: mpsc::UnboundedSender<String>)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
}

fn set(status: &watch::Sender<PreviewStatus>, side: Side, state: ServerState, url: Option<String>) {
    status.send_if_modified(|s| {
        let slot = match side {
            Side::Frontend => &mut s.frontend,
            Side::Backend => &mut s.backend,
        };
        let mut changed = *slot != state;
        *slot = state;
        if side == Side::Frontend {
            // The page stays shown during rebuilds (the server keeps serving the
            // last good build); it goes away when the server stops.
            let new_url = if url.is_some() {
                url
            } else if s.frontend == ServerState::Off {
                None
            } else {
                s.url.clone()
            };
            changed |= new_url != s.url;
            s.url = new_url;
        }
        changed
    });
}

/// Follows one process's output and exit.
struct Watcher {
    side: Side,
    status: watch::Sender<PreviewStatus>,
    plan: PreviewPlan,
    page_url: String,
    stopping: Arc<AtomicBool>,
}

impl Watcher {
    async fn run(self, mut child: tokio::process::Child, mut rx: mpsc::UnboundedReceiver<String>) {
        let mut tracker = BuildTracker::new(self.plan.frontend_log);
        let mut tail = Tail::default();
        let mut ready = false;
        let ReadySignal::StdoutPrefix(ready_prefix) = self.plan.backend_ready;
        set(&self.status, self.side, ServerState::Building, None);
        loop {
            tokio::select! {
                line = rx.recv() => {
                    let Some(line) = line else {
                        // Output closed: the process is ending.
                        let code = child.wait().await.ok().and_then(|s| s.code());
                        self.exited(code, &tail, ready);
                        return;
                    };
                    let clean = tail.push(&line);
                    match self.side {
                        Side::Frontend => {
                            if let Some(state) = tracker.feed(&clean) {
                                let url = state.is_ready().then(|| self.page_url.clone());
                                set(&self.status, self.side, state, url);
                            }
                        }
                        Side::Backend => {
                            if !ready && clean.starts_with(ready_prefix) {
                                ready = true;
                                set(&self.status, self.side, ServerState::Ready, None);
                            }
                        }
                    }
                }
                status = child.wait() => {
                    // Read what is left of the output first.
                    while let Ok(line) = rx.try_recv() {
                        tail.push(&line);
                    }
                    self.exited(status.ok().and_then(|s| s.code()), &tail, ready);
                    return;
                }
            }
        }
    }

    fn exited(&self, code: Option<i32>, tail: &Tail, was_ready: bool) {
        if self.stopping.load(Ordering::SeqCst) {
            return;
        }
        let what = match (self.side, was_ready) {
            (Side::Backend, true) => "The app stopped",
            (Side::Backend, false) => "The app could not be built or started",
            (Side::Frontend, _) => "The preview server stopped",
        };
        let code = code.map_or_else(|| "no exit code".to_owned(), |c| format!("exit code {c}"));
        set(
            &self.status,
            self.side,
            ServerState::Error {
                output: format!("{what} ({code}).\n{}", tail.text()),
            },
            None,
        );
    }
}
