//! The preview manager against a fake trunk/backend (src/bin/fake_tool.rs):
//! states, rebuilds, compiler errors, backend restarts and stopping.

// Test code: clippy's allow-unwrap-in-tests does not cover helper fns here.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::time::Duration;

use ak_preview::{Preview, Tools, profiles};
use ak_types::preview::{PreviewStatus, ServerState};
use tokio::sync::watch;

const FAKE: &str = env!("CARGO_BIN_EXE_ak-preview-fake-tool");

struct Setup {
    dir: tempfile::TempDir,
    preview: Preview,
}

impl Setup {
    fn new(front: &str, back: &str) -> Setup {
        let dir = tempfile::tempdir().unwrap();
        let trigger = dir.path().join("trigger");
        std::fs::write(&trigger, "").unwrap();
        let tools = Tools {
            cargo: PathBuf::from(FAKE),
            trunk: PathBuf::from(FAKE),
            cargo_tauri: PathBuf::from(FAKE),
            env: vec![
                ("FAKE_FRONT".into(), front.into()),
                ("FAKE_BACK".into(), back.into()),
                ("FAKE_TRIGGER".into(), trigger.display().to_string()),
                (
                    "FAKE_LOG".into(),
                    dir.path().join("log").display().to_string(),
                ),
            ],
            path_prefix: vec![],
        };
        let plan = profiles::default_profile().preview();
        let preview = Preview::new(dir.path(), plan, tools).unwrap();
        Setup { dir, preview }
    }

    fn trigger(&self, what: &str) {
        std::fs::write(self.dir.path().join("trigger"), what).unwrap();
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("log")).unwrap_or_default()
    }
}

/// Waits until the status satisfies `ok` (or fails after 15 s).
async fn wait(
    rx: &mut watch::Receiver<PreviewStatus>,
    what: &str,
    ok: impl Fn(&PreviewStatus) -> bool,
) -> PreviewStatus {
    let result = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if ok(&rx.borrow_and_update()) {
                return rx.borrow().clone();
            }
            rx.changed().await.unwrap();
        }
    })
    .await;
    result.unwrap_or_else(|_| panic!("timed out waiting for {what}: {:?}", rx.borrow()))
}

fn error_output(s: &ServerState) -> &str {
    match s {
        ServerState::Error { output } => output,
        other => panic!("expected an error, got {other:?}"),
    }
}

async fn page_answers(url: &str) -> bool {
    let rest = url.strip_prefix("http://").unwrap();
    let host = rest.split('/').next().unwrap();
    let Ok(mut s) = tokio::net::TcpStream::connect(host).await else {
        return false;
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    s.write_all(b"GET / HTTP/1.0\r\n\r\n").await.unwrap();
    let mut buf = Vec::new();
    let _ = s.read_to_end(&mut buf).await;
    String::from_utf8_lossy(&buf).starts_with("HTTP/1.1 200")
}

#[tokio::test(flavor = "multi_thread")]
async fn starts_both_and_follows_rebuilds() {
    let mut s = Setup::new("ok", "ok");
    let mut rx = s.preview.subscribe();
    s.preview.start().unwrap();
    let st = wait(&mut rx, "ready", PreviewStatus::ready).await;

    // The page URL carries the bridge address; the backend got the same values.
    let url = st.url.clone().unwrap();
    assert!(url.contains("?ak_bridge=ws%3A%2F%2F127.0.0.1%3A"), "{url}");
    assert!(page_answers(&url).await, "the dev server serves {url}");
    let log = s.log();
    assert!(log.contains("serve port="), "{log}");
    assert!(log.contains("--features dev"), "{log}");
    let run = log.lines().find(|l| l.starts_with("run ")).unwrap();
    assert!(!run.contains('{'), "placeholders filled: {run}");
    assert!(
        run.contains(&format!("origin={}", url.split("/?").next().unwrap())),
        "{run}"
    );
    let token = run
        .split("token=")
        .nth(1)
        .unwrap()
        .split(' ')
        .next()
        .unwrap();
    assert_eq!(token.len(), 32);
    assert!(
        s.preview
            .controller_url()
            .contains(&format!("token={token}&role=controller"))
    );

    // A failing rebuild: error with the compiler output; the page stays up.
    s.trigger("fail");
    let st = wait(&mut rx, "frontend error", |st| st.frontend.is_error()).await;
    let out = error_output(&st.frontend);
    assert!(out.contains("error[E0308]: mismatched types"), "{out}");
    assert!(out.contains("src/main.rs:3:5"), "{out}");
    assert_eq!(
        st.url.as_deref(),
        Some(url.as_str()),
        "the last good build stays shown"
    );
    assert_eq!(st.error(), Some(out));

    // Fixed: ready again.
    s.trigger("ok");
    wait(&mut rx, "ready again", PreviewStatus::ready).await;

    s.preview.stop().await;
    let st = s.preview.status();
    assert_eq!(
        (st.frontend, st.backend, st.url),
        (ServerState::Off, ServerState::Off, None)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn backend_compile_errors_and_crashes_are_reported() {
    let mut s = Setup::new("ok", "fail");
    let mut rx = s.preview.subscribe();
    s.preview.start().unwrap();
    let st = wait(&mut rx, "backend error", |st| st.backend.is_error()).await;
    let out = error_output(&st.backend);
    assert!(
        out.starts_with("The app could not be built or started"),
        "{out}"
    );
    assert!(out.contains("error[E0425]: cannot find value `x`"), "{out}");
    s.preview.stop().await;

    let mut s = Setup::new("ok", "crash");
    let mut rx = s.preview.subscribe();
    s.preview.start().unwrap();
    let st = wait(&mut rx, "crash", |st| st.backend.is_error()).await;
    let out = error_output(&st.backend);
    assert!(out.starts_with("The app stopped"), "{out}");
    assert!(out.contains("panicked at src/main.rs:9:5: boom"), "{out}");
    s.preview.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn backend_restarts_only_for_backend_code_and_keeps_its_port() {
    let mut s = Setup::new("ok", "ok");
    let mut rx = s.preview.subscribe();
    s.preview.start().unwrap();
    wait(&mut rx, "ready", PreviewStatus::ready).await;

    assert!(
        !s.preview
            .files_changed(&["src/main.rs".into()])
            .await
            .unwrap()
    );
    assert!(
        s.preview
            .files_changed(&["src-tauri\\src\\main.rs".into()])
            .await
            .unwrap()
    );
    wait(&mut rx, "backend ready again", PreviewStatus::ready).await;
    let starts: Vec<String> = s
        .log()
        .lines()
        .filter(|l| l.starts_with("run "))
        .map(str::to_owned)
        .collect();
    assert_eq!(starts.len(), 2, "{starts:?}");
    assert_eq!(
        starts[0], starts[1],
        "same port and token after the restart"
    );
    // Not a prefix match on names: src-tauri-notes is not backend code.
    assert!(
        !s.preview
            .files_changed(&["src-tauri-notes/x".into()])
            .await
            .unwrap()
    );
    s.preview.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dying_dev_server_is_an_error_and_drop_stops_everything() {
    let mut s = Setup::new("ok", "ok");
    let mut rx = s.preview.subscribe();
    s.preview.start().unwrap();
    wait(&mut rx, "ready", PreviewStatus::ready).await;
    s.trigger("exit");
    let st = wait(&mut rx, "frontend stopped", |st| st.frontend.is_error()).await;
    assert!(error_output(&st.frontend).starts_with("The preview server stopped (exit code 3)"));

    // Dropping the preview stops the backend's whole process tree.
    let url = st.url.clone();
    drop(s.preview);
    tokio::time::sleep(Duration::from_millis(500)).await;
    if let Some(url) = url {
        assert!(!page_answers(&url).await);
    }
}
