//! The open project's files, history, preview status and real window.

use std::path::{Path, PathBuf};

use ak_agent::tools::path::resolve;
use ak_project::{CheckpointKind, Timeline};
use ak_types::ipc::{FileEntry, FileText, MarkVersion, SaveFile, TimelineRow};
use ak_types::preview::PreviewStatus;
use ak_types::stack::{CommandSpec, Tool};
use tauri::State;

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

/// "Run in a real window": the app as it runs installed (no preview bridge),
/// through `cargo tauri dev` with AgentKnife's toolchain. A second click
/// replaces the first window.
#[tauri::command]
pub async fn run_app(state: State<'_, AppState>) -> Res<()> {
    let mut inner = state.inner.lock().await;
    let open = inner
        .open
        .as_mut()
        .ok_or_else(|| fail("no project is open"))?;
    if let Some(mut old) = open.app_window.take() {
        let _ = old.start_kill();
    }
    let spec = CommandSpec {
        tool: Tool::CargoTauri,
        args: vec!["dev".into()],
        cwd: "",
        env: vec![("TRUNK_SKIP_VERSION_CHECK".into(), "true".into())],
        env_remove: vec!["NO_COLOR".into()],
    };
    let mut cmd = state.tools.command(&spec, &open.root, &[]);
    cmd.stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    open.app_window = Some(cmd.spawn().map_err(fail)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
