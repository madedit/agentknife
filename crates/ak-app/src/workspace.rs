//! The open project's files, history, preview status and real window.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ak_agent::tools::path::resolve;
use ak_preview::log::{strip_ansi, tail};
use ak_preview::process::{kill_tree, read_lines};
use ak_project::{CheckpointKind, Timeline};
use ak_types::ipc::{FileEntry, FileText, MarkVersion, SaveFile, TimelineRow};
use ak_types::log::{LogLine, LogSource};
use ak_types::preview::{Picked, PreviewFrame, PreviewStatus};
use ak_types::stack::{CommandSpec, Tool};
use tauri::State;
use tokio::sync::mpsc;

use crate::state::AppState;
use crate::{Res, fail};

/// Folders the files pane never shows (AgentKnife's own, git, the vendored kit).
const HIDDEN: &[&str] = &[".agentknife", ".git", "ak"];
/// Bytes read to decide whether a file is text.
const SNIFF: usize = 8_000;

async fn root(state: &AppState) -> Res<PathBuf> {
    state
        .inner
        .lock()
        .await
        .open
        .as_ref()
        .map(|o| o.root.clone())
        .ok_or_else(|| fail("no project is open"))
}

/// The project's files and folders, `/`-separated and sorted; ignored files
/// (.gitignore: build output) and [`HIDDEN`] folders are left out.
pub fn list(root: &Path) -> Vec<FileEntry> {
    let mut out: Vec<FileEntry> = ignore::WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .require_git(false)
        .filter_entry(|e| {
            !(e.depth() == 1
                && e.file_type().is_some_and(|t| t.is_dir())
                && HIDDEN.iter().any(|h| e.file_name() == *h))
        })
        .build()
        .filter_map(Result::ok)
        .filter(|e| e.depth() > 0)
        .filter_map(|e| {
            let rel = e.path().strip_prefix(root).ok()?;
            Some(FileEntry {
                path: rel.to_string_lossy().replace('\\', "/"),
                dir: e.file_type().is_some_and(|t| t.is_dir()),
            })
        })
        .collect();
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// Text, or `None` for binary content (a NUL byte early on, or not UTF-8).
pub fn as_text(bytes: Vec<u8>) -> Option<String> {
    if bytes.iter().take(SNIFF).any(|b| *b == 0) {
        return None;
    }
    String::from_utf8(bytes).ok()
}

#[tauri::command]
pub async fn list_files(state: State<'_, AppState>) -> Res<Vec<FileEntry>> {
    let root = root(&state).await?;
    tokio::task::spawn_blocking(move || list(&root))
        .await
        .map_err(fail)
}

#[tauri::command]
pub async fn read_file(state: State<'_, AppState>, request: String) -> Res<FileText> {
    let root = root(&state).await?;
    let path = resolve(&root, &request).map_err(fail)?;
    let bytes = tokio::fs::read(&path).await.map_err(fail)?;
    Ok(match as_text(bytes) {
        Some(text) => FileText {
            path: request,
            text,
            binary: false,
        },
        None => FileText {
            path: request,
            text: String::new(),
            binary: true,
        },
    })
}

#[tauri::command]
pub async fn write_file(state: State<'_, AppState>, request: SaveFile) -> Res<()> {
    let (root, preview) = {
        let inner = state.inner.lock().await;
        let open = inner
            .open
            .as_ref()
            .ok_or_else(|| fail("no project is open"))?;
        (open.root.clone(), open.preview.clone())
    };
    let path = resolve(&root, &request.path).map_err(fail)?;
    tokio::fs::write(&path, request.text).await.map_err(fail)?;
    let _ = preview.files_changed(&[request.path]).await;
    Ok(())
}

#[tauri::command]
pub async fn timeline(state: State<'_, AppState>) -> Res<Vec<TimelineRow>> {
    let root = root(&state).await?;
    tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<TimelineRow>> {
        let t = Timeline::open(&root)?;
        let last = t.entries().len().saturating_sub(1);
        Ok(t.entries()
            .iter()
            .enumerate()
            .rev()
            .map(|(n, e)| TimelineRow {
                id: e.id.clone(),
                kind: e
                    .kind
                    .message_id()
                    .trim_start_matches("timeline-")
                    .to_owned(),
                label: e.label.clone(),
                created_at: e.created_at,
                changed_files: e.changed_files.clone(),
                current: n == last,
            })
            .collect())
    })
    .await
    .map_err(fail)?
    .map_err(fail)
}

#[tauri::command]
pub async fn restore(state: State<'_, AppState>, request: String) -> Res<()> {
    let (root, preview) = {
        let inner = state.inner.lock().await;
        let open = inner
            .open
            .as_ref()
            .ok_or_else(|| fail("no project is open"))?;
        if open.cancel.is_some() {
            return Err(fail("the AI is working; stop it first"));
        }
        (open.root.clone(), open.preview.clone())
    };
    let changed = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<String>> {
        let mut t = Timeline::open(&root)?;
        let label = t
            .entries()
            .iter()
            .find(|e| e.id == request)
            .map(|e| e.label.clone())
            .unwrap_or_default();
        Ok(t.restore(&request, &label)?.changed_files)
    })
    .await
    .map_err(fail)?
    .map_err(fail)?;
    let _ = preview.files_changed(&changed).await;
    Ok(())
}

#[tauri::command]
pub async fn mark_version(state: State<'_, AppState>, request: MarkVersion) -> Res<()> {
    let root = root(&state).await?;
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        Timeline::open(&root)?.checkpoint(CheckpointKind::Marked, request.label.trim(), None)?;
        Ok(())
    })
    .await
    .map_err(fail)?
    .map_err(fail)
}

/// What the user points at in the preview, as soon as they pick it.
#[tauri::command]
pub async fn set_selection(state: State<'_, AppState>, request: Option<Picked>) -> Res<()> {
    if let Some(o) = state.inner.lock().await.open.as_ref() {
        o.preview.set_selection(request);
    }
    Ok(())
}

/// Where the preview page shows in the window now, for screenshots.
#[tauri::command]
pub fn set_preview_frame(state: State<'_, AppState>, request: Option<PreviewFrame>) -> Res<()> {
    if let Ok(mut f) = state.frame.lock() {
        *f = request;
    }
    Ok(())
}

/// "Clear data and start over": deletes what the app saved in the preview and
/// starts the preview again as on its first run. Never while the AI changes
/// the project. The real window shares that folder, so it closes.
#[tauri::command]
pub async fn clear_preview_data(state: State<'_, AppState>) -> Res<()> {
    let preview = {
        let mut inner = state.inner.lock().await;
        let open = inner
            .open
            .as_mut()
            .ok_or_else(|| fail("no project is open"))?;
        if open.cancel.is_some() || crate::mcp::busy(&state) {
            return Err(fail("the AI is working; try again when it is done"));
        }
        if let Some(window) = open.app_window.take()
            && let Some(pid) = window.id()
        {
            kill_tree(pid).await;
        }
        open.log.push(
            LogSource::Screens,
            "> clear the preview's data and start over",
        );
        open.preview.clone()
    };
    preview.clear_data().await.map_err(fail)
}

#[tauri::command]
pub async fn preview_status(state: State<'_, AppState>) -> Res<PreviewStatus> {
    Ok(state
        .inner
        .lock()
        .await
        .open
        .as_ref()
        .map(|o| o.preview.status())
        .unwrap_or_default())
}

/// The open project's log so far (the Log tab); new lines follow as `LOG_EVENT`.
#[tauri::command]
pub async fn get_log(state: State<'_, AppState>) -> Res<Vec<LogLine>> {
    Ok(state
        .inner
        .lock()
        .await
        .open
        .as_ref()
        .map(|o| o.log.lines())
        .unwrap_or_default())
}

#[tauri::command]
pub async fn clear_log(state: State<'_, AppState>) -> Res<()> {
    if let Some(o) = state.inner.lock().await.open.as_ref() {
        o.log.clear();
    }
    Ok(())
}

/// Where "Run in a real window" builds the app, under the project's `target/`.
/// Not `target/debug`: the preview runs its own `app.exe` from there, and Windows
/// cannot replace a running program.
const WINDOW_TARGET: &str = "ak-window";
/// The window counts as open when the app still runs this long after starting.
const WINDOW_SETTLE: Duration = Duration::from_secs(3);
/// Output lines kept for Details.
const WINDOW_LINES: usize = 80;

/// cargo's line as it starts the built app (`Running `path``), not tauri's
/// `Running DevCommand (…)`.
fn app_started(line: &str) -> bool {
    line.trim_start().starts_with("Running `")
}

/// cargo's last word on a failed build. `cargo tauri dev` does not exit then: it
/// waits for the files to change.
fn build_failed(line: &str) -> bool {
    line.starts_with("error: could not compile") || line.starts_with("error: failed to")
}

/// "Run in a real window": the app as it runs installed (no preview bridge),
/// through `cargo tauri dev` with AgentKnife's toolchain. Returns once the app
/// has been running for a moment, or with the output when it could not start (a
/// first build takes minutes). A second click replaces the first window.
#[tauri::command]
pub async fn run_app(state: State<'_, AppState>) -> Res<()> {
    let (pid, mut lines) = {
        let mut inner = state.inner.lock().await;
        let open = inner
            .open
            .as_mut()
            .ok_or_else(|| fail("no project is open"))?;
        if let Some(old) = open.app_window.take()
            && let Some(pid) = old.id()
        {
            kill_tree(pid).await;
        }
        let target = open.root.join("target").join(WINDOW_TARGET);
        // Still a test: the window keeps its data where the preview does, never
        // in the installed app's folders.
        let data = open
            .root
            .join(crate::home::profile_of_root(open).preview().data_dir);
        let config = serde_json::json!({ "app": { "appDirectoriesOverride": data } });
        let spec = CommandSpec {
            tool: Tool::CargoTauri,
            // After `--`: arguments for `cargo run`.
            args: vec![
                "dev".into(),
                "--config".into(),
                config.to_string(),
                "--".into(),
                "--target-dir".into(),
                target.to_string_lossy().into_owned(),
            ],
            cwd: "",
            env: std::iter::once(("TRUNK_SKIP_VERSION_CHECK", "true"))
                .chain(ak_preview::process::CARGO_PROGRESS_ENV)
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect(),
            env_remove: vec!["NO_COLOR".into()],
        };
        let (reader, writer) = std::io::pipe().map_err(fail)?;
        let mut cmd = state.tools.command(&spec, &open.root, &[]);
        cmd.stdout(writer.try_clone().map_err(fail)?).stderr(writer);
        let child = cmd.spawn().map_err(fail)?;
        // Our copies of the write end must close, or the reader never ends.
        drop(cmd);
        let (tx, mut raw) = mpsc::unbounded_channel();
        read_lines(reader, tx);
        // Read for as long as the app runs (into the log), not only until this
        // command returns: a closed pipe would break `cargo tauri dev`.
        let log = open.log.clone();
        log.push(LogSource::Window, "> cargo tauri dev");
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Some(line) = raw.recv().await {
                log.push(LogSource::Window, &line);
                let _ = tx.send(line);
            }
        });
        let pid = child.id();
        open.app_window = Some(child);
        (pid, rx)
    };
    let mut output = VecDeque::new();
    let mut started: Option<Instant> = None;
    let mut failed: Option<Instant> = None;
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    loop {
        tokio::select! {
            line = lines.recv(), if !lines.is_closed() => {
                let Some(line) = line else { continue };
                // cargo redraws its progress bar with CR: one read may hold the
                // bar and then the line that matters.
                for part in line.split('\r').map(strip_ansi) {
                    if part.trim().is_empty() || part.trim_start().starts_with("Building [") {
                        continue;
                    }
                    if started.is_none() && app_started(&part) {
                        started = Some(Instant::now());
                    }
                    if failed.is_none() && build_failed(&part) {
                        failed = Some(Instant::now());
                    }
                    output.push_back(part);
                    if output.len() > WINDOW_LINES {
                        output.pop_front();
                    }
                }
            }
            _ = tick.tick() => {
                let mut inner = state.inner.lock().await;
                let Some(child) = inner.open.as_mut().and_then(|o| o.app_window.as_mut()) else {
                    return Ok(()); // the project was closed
                };
                if child.id() != pid {
                    return Ok(()); // replaced by another click
                }
                if child.try_wait().map_err(fail)?.is_some() {
                    while let Ok(line) = lines.try_recv() {
                        output.push_back(strip_ansi(&line));
                    }
                    return Err(fail(tail(&output)));
                }
                // A moment for the last lines, then nothing more will come.
                if failed.is_some_and(|t| t.elapsed() >= Duration::from_secs(1)) {
                    if let Some(pid) = pid {
                        kill_tree(pid).await;
                    }
                    while let Ok(line) = lines.try_recv() {
                        output.push_back(strip_ansi(&line));
                    }
                    return Err(fail(tail(&output)));
                }
                if started.is_some_and(|t| t.elapsed() >= WINDOW_SETTLE) {
                    return Ok(());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_counts_from_cargo_starting_the_app_not_from_tauri() {
        assert!(app_started(
            r"     Running `C:\p\target\ak-window\debug\app.exe`"
        ));
        assert!(!app_started(
            "     Running DevCommand (`cargo  run --target-dir x --no-default-features --`)"
        ));
        assert!(!app_started(
            "     Running BeforeDevCommand (`trunk serve`)"
        ));
    }

    #[test]
    fn a_failed_build_is_seen_from_cargos_last_word() {
        assert!(build_failed(
            "error: could not compile `app` (bin \"app\") due to 1 previous error"
        ));
        assert!(build_failed(
            r"error: failed to remove file `target\debug\app.exe`"
        ));
        assert!(!build_failed("error[E0308]: mismatched types"));
        assert!(!build_failed("   Compiling app v0.1.0"));
    }

    #[test]
    fn the_file_list_hides_build_output_and_agentknife() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let r = dir.path();
        for f in [
            "Cargo.toml",
            "src/main.rs",
            "target/debug/app.exe",
            ".agentknife/conversation.json",
            "ak/ak-ui/src/lib.rs",
            "dist/index.html",
        ] {
            let p = r.join(f);
            std::fs::create_dir_all(p.parent().unwrap_or(r)).unwrap_or_else(|e| panic!("{e}"));
            std::fs::write(p, "x").unwrap_or_else(|e| panic!("{e}"));
        }
        std::fs::write(r.join(".gitignore"), "/target/\n/dist/\n")
            .unwrap_or_else(|e| panic!("{e}"));
        let paths: Vec<String> = list(r).into_iter().map(|e| e.path).collect();
        assert_eq!(paths, [".gitignore", "Cargo.toml", "src", "src/main.rs"]);
    }

    #[test]
    fn binary_files_are_recognized() {
        assert_eq!(
            as_text(b"fn main() {}".to_vec()).as_deref(),
            Some("fn main() {}")
        );
        assert_eq!(as_text(vec![0x89, b'P', b'N', b'G', 0, 1]), None);
        assert_eq!(as_text(vec![0xff, 0xfe, 0x41]), None);
    }
}
