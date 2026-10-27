//! The user's own files in the project (P1-SHELL-007): pictures, 3D models,
//! sounds and fonts copied into `assets/<kind>/`, served by trunk at
//! `./assets/…`. The AI never downloads such files; the user adds them here
//! and the AI hears of them with their next message (and over MCP in
//! `status`).

use std::path::{Path, PathBuf};

use ak_project::{CheckpointKind, Timeline};
use ak_types::i18n::{I18n, args};
use ak_types::ipc::{
    AddFiles, AddedFile, AddedFiles, AssetKind, FailedFile, HeldFile, HoldReason, NewFile,
    PROJECT_CHANGED_EVENT,
};
use base64::Engine;
use tauri::{AppHandle, Emitter, Runtime, State};
use tauri_plugin_dialog::DialogExt;

use crate::state::AppState;
use crate::{Res, fail, ui_locale};

/// The project folder the files go to.
pub const ASSETS: &str = "assets";
/// Larger files are copied only after the user agrees.
pub const LARGE: u64 = 50 * 1024 * 1024;
/// How many added files the AI is told about at most (newest).
const NOTED: usize = 30;

/// What a file is, from its name.
pub fn kind_of(name: &str) -> AssetKind {
    let ext = Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "ico" | "avif" => {
            AssetKind::Image
        }
        "glb" | "gltf" | "bin" | "obj" | "mtl" | "fbx" | "stl" => AssetKind::Model,
        "mp3" | "wav" | "ogg" | "oga" | "m4a" | "aac" | "flac" | "opus" => AssetKind::Sound,
        "ttf" | "otf" | "woff" | "woff2" => AssetKind::Font,
        _ => AssetKind::Other,
    }
}

/// The folder under `assets/` for a kind.
pub fn folder(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::Image => "images",
        AssetKind::Model => "models",
        AssetKind::Sound => "sounds",
        AssetKind::Font => "fonts",
        AssetKind::Other => "files",
    }
}

/// Plain words for the AI.
fn kind_words(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::Image => "picture",
        AssetKind::Model => "3D model",
        AssetKind::Sound => "sound",
        AssetKind::Font => "font",
        AssetKind::Other => "file",
    }
}

/// A file name alone (no folders), or `None` when there is none.
fn file_name(raw: &str) -> Option<String> {
    // Dropped names come from the window: never let them climb out.
    let name = Path::new(raw.trim())
        .file_name()?
        .to_string_lossy()
        .into_owned();
    (!name.is_empty() && name != "." && name != "..").then_some(name)
}

/// `name` in `dir`, numbered when taken: `cat.png`, `cat (2).png`, …
pub fn free_name(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let p = Path::new(name);
    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = p
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

/// `index.html` with the trunk link that serves `assets/`, or `None` when it
/// has one already.
pub fn with_copy_dir(index_html: &str) -> Option<String> {
    let has = index_html.lines().any(|l| {
        l.contains("data-trunk")
            && l.contains("copy-dir")
            && (l.contains("href=\"assets\"") || l.contains("href=\"assets/\""))
    });
    if has {
        return None;
    }
    const LINK: &str = "<link data-trunk rel=\"copy-dir\" href=\"assets\" />";
    let mut out = String::with_capacity(index_html.len() + LINK.len() + 8);
    let mut done = false;
    for line in index_html.split_inclusive('\n') {
        // Before the app's own link (or the end of <head>), indented like it.
        if !done && (line.contains("rel=\"rust\"") || line.contains("</head>")) {
            let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
            let indent = if line.contains("</head>") {
                format!("{indent}  ")
            } else {
                indent
            };
            out.push_str(&format!("{indent}{LINK}\n"));
            done = true;
        }
        out.push_str(line);
    }
    done.then_some(out)
}

enum Source {
    Disk(PathBuf),
    Bytes(Vec<u8>),
}

/// Copies `files` into `root/assets/<kind>/`. Large files and unknown kinds
/// are held unless `allow`. Returns what happened and the project files it
/// changed (for the preview).
pub fn add(root: &Path, files: &[NewFile], allow: bool) -> (AddedFiles, Vec<String>) {
    let mut out = AddedFiles::default();
    let mut touched = Vec::new();
    for (index, file) in files.iter().enumerate() {
        let raw_name = match file {
            NewFile::Path { path } => path.as_str(),
            NewFile::Dropped { name, .. } => name.as_str(),
        };
        let Some(name) = file_name(raw_name) else {
            out.failed.push(FailedFile {
                name: raw_name.to_owned(),
                error: "not a file name".into(),
            });
            continue;
        };
        let failed = |out: &mut AddedFiles, error: String| {
            out.failed.push(FailedFile {
                name: name.clone(),
                error,
            });
        };
        let (source, size) = match file {
            NewFile::Path { path } => {
                let path = PathBuf::from(path);
                match std::fs::metadata(&path) {
                    Ok(m) if m.is_file() => (Source::Disk(path), m.len()),
                    Ok(_) => {
                        failed(&mut out, "a folder, not a file".into());
                        continue;
                    }
                    Err(e) => {
                        failed(&mut out, e.to_string());
                        continue;
                    }
                }
            }
            NewFile::Dropped { data_base64, .. } => {
                match base64::engine::general_purpose::STANDARD.decode(data_base64) {
                    Ok(bytes) => {
                        let size = bytes.len() as u64;
                        (Source::Bytes(bytes), size)
                    }
                    Err(e) => {
                        failed(&mut out, format!("unreadable contents: {e}"));
                        continue;
                    }
                }
            }
        };
        let kind = kind_of(&name);
        let reason = if size > LARGE {
            Some(HoldReason::Large)
        } else if kind == AssetKind::Other {
            Some(HoldReason::Unknown)
        } else {
            None
        };
        if let Some(reason) = reason.filter(|_| !allow) {
            out.held.push(HeldFile {
                index,
                name: name.clone(),
                size,
                reason,
            });
            continue;
        }
        let dir = root.join(ASSETS).join(folder(kind));
        if let Err(e) = std::fs::create_dir_all(&dir) {
            failed(&mut out, e.to_string());
            continue;
        }
        let target = free_name(&dir, &name);
        let copied = match source {
            Source::Disk(from) => std::fs::copy(from, &target).map(|_| ()),
            Source::Bytes(bytes) => std::fs::write(&target, bytes),
        };
        if let Err(e) = copied {
            failed(&mut out, e.to_string());
            continue;
        }
        let rel = format!(
            "{ASSETS}/{}/{}",
            folder(kind),
            target
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        );
        touched.push(rel.clone());
        out.added.push(AddedFile {
            path: rel,
            kind,
            size,
        });
    }
    // The preview and the packaged app serve `assets/` only with this link.
    if !out.added.is_empty() {
        let index = root.join("index.html");
        if let Some(new) = std::fs::read_to_string(&index)
            .ok()
            .and_then(|t| with_copy_dir(&t))
            && std::fs::write(&index, new).is_ok()
        {
            touched.push("index.html".into());
        }
    }
    (out, touched)
}

/// One line per file for the AI.
pub fn describe(files: &[AddedFile]) -> String {
    files
        .iter()
        .rev()
        .take(NOTED)
        .rev()
        .map(|f| {
            format!(
                "- {} ({}, {} KB)",
                f.path,
                kind_words(f.kind),
                f.size.div_ceil(1024)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// What the AI hears with the next message about files the user added.
pub fn note(files: &[AddedFile]) -> Option<String> {
    (!files.is_empty()).then(|| {
        format!(
            "The user added these files to the project (copied into `assets/`, which the \
             app loads as `./assets/…`; index.html serves the folder). Use them where they \
             belong; do not rename or move them:\n{}",
            describe(files)
        )
    })
}

/// The version's name in the user's language: "Added files: cat.png, …".
fn version_label(files: &[AddedFile]) -> String {
    let names: Vec<&str> = files
        .iter()
        .map(|f| f.path.rsplit('/').next().unwrap_or(&f.path))
        .collect();
    let mut shown = names.iter().take(3).copied().collect::<Vec<_>>().join(", ");
    if names.len() > 3 {
        shown.push_str(", …");
    }
    I18n::new(ui_locale())
        .map(|i| i.t_args("files-added-version", &args([("names", shown.clone())])))
        .unwrap_or(shown)
}

#[tauri::command]
pub async fn pick_files<R: Runtime>(app: AppHandle<R>) -> Res<Option<Vec<String>>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_files(move |picked| {
        let _ = tx.send(picked);
    });
    let Some(picked) = rx.await.ok().flatten() else {
        return Ok(None);
    };
    let paths = picked
        .into_iter()
        .filter_map(|p| p.into_path().ok())
        .map(|p| p.display().to_string())
        .collect();
    Ok(Some(paths))
}

#[tauri::command]
pub async fn add_files<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    request: AddFiles,
) -> Res<AddedFiles> {
    let (root, preview, working) = {
        let inner = state.inner.lock().await;
        let open = inner
            .open
            .as_ref()
            .ok_or_else(|| fail("no project is open"))?;
        (
            open.root.clone(),
            open.preview.clone(),
            open.cancel.is_some(),
        )
    };
    let (result, touched) = {
        let root = root.clone();
        tokio::task::spawn_blocking(move || add(&root, &request.files, request.allow))
            .await
            .map_err(fail)?
    };
    if result.added.is_empty() {
        return Ok(result);
    }
    {
        let mut inner = state.inner.lock().await;
        if let Some(open) = inner.open.as_mut().filter(|o| o.root == root) {
            open.added_files.extend(result.added.iter().cloned());
            open.unannounced.extend(result.added.iter().cloned());
        }
    }
    let _ = preview.files_changed(&touched).await;
    // A version to go back to, unless an AI is changing the project now (its
    // own version takes the files along).
    if !working && !crate::mcp::busy(&state) {
        let label = version_label(&result.added);
        let _ = tokio::task::spawn_blocking(move || {
            Timeline::open(&root).and_then(|mut t| t.checkpoint(CheckpointKind::Turn, &label, None))
        })
        .await;
    }
    let _ = app.emit(PROJECT_CHANGED_EVENT, ());
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dropped(name: &str, bytes: &[u8]) -> NewFile {
        NewFile::Dropped {
            name: name.into(),
            data_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        }
    }

    #[test]
    fn kinds_and_folders() {
        assert_eq!(kind_of("Cat.PNG"), AssetKind::Image);
        assert_eq!(kind_of("plane.glb"), AssetKind::Model);
        assert_eq!(kind_of("song.mp3"), AssetKind::Sound);
        assert_eq!(kind_of("title.woff2"), AssetKind::Font);
        assert_eq!(kind_of("notes.docx"), AssetKind::Other);
        assert_eq!(kind_of("README"), AssetKind::Other);
        assert_eq!(folder(AssetKind::Sound), "sounds");
    }

    #[test]
    fn files_are_copied_by_kind_numbered_and_held_when_asked() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("index.html"),
            "<head>\n    <title>x</title>\n    <link data-trunk rel=\"rust\" data-bin=\"app-ui\" />\n</head>\n",
        )
        .unwrap();
        let outside = tempfile::tempdir().unwrap();
        let picked = outside.path().join("song.mp3");
        std::fs::write(&picked, b"ID3").unwrap();

        let files = vec![
            NewFile::Path {
                path: picked.display().to_string(),
            },
            dropped("cat.png", b"png"),
            dropped("cat.png", b"png2"),
            dropped("notes.docx", b"doc"),
            dropped("../../evil.png", b"x"),
            NewFile::Path {
                path: outside.path().join("missing.png").display().to_string(),
            },
        ];
        let (out, touched) = add(root, &files, false);
        let paths: Vec<&str> = out.added.iter().map(|a| a.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "assets/sounds/song.mp3",
                "assets/images/cat.png",
                "assets/images/cat (2).png",
                "assets/images/evil.png",
            ]
        );
        assert_eq!(std::fs::read(picked).unwrap(), b"ID3", "the original stays");
        assert_eq!(
            std::fs::read(root.join("assets/images/cat (2).png")).unwrap(),
            b"png2"
        );
        assert_eq!(out.held.len(), 1);
        assert_eq!(
            (out.held[0].index, out.held[0].reason),
            (3, HoldReason::Unknown)
        );
        assert_eq!(out.failed.len(), 1, "{:?}", out.failed);
        assert!(touched.contains(&"index.html".to_owned()));
        let html = std::fs::read_to_string(root.join("index.html")).unwrap();
        assert!(
            html.contains("    <link data-trunk rel=\"copy-dir\" href=\"assets\" />\n    <link data-trunk rel=\"rust\""),
            "{html}"
        );

        // The held one, once the user agrees; the link is not added twice.
        let (again, touched) = add(root, &[files[3].clone()], true);
        assert_eq!(again.added[0].path, "assets/files/notes.docx");
        assert!(!touched.contains(&"index.html".to_owned()));
        assert_eq!(
            std::fs::read_to_string(root.join("index.html"))
                .unwrap()
                .matches("copy-dir")
                .count(),
            1
        );
    }

    #[test]
    fn the_link_goes_before_the_app_or_the_end_of_head() {
        assert_eq!(
            with_copy_dir(
                "<head>\n  <link data-trunk rel=\"copy-dir\" href=\"assets\" />\n</head>"
            ),
            None
        );
        let no_rust = with_copy_dir("<head>\n  <title>x</title>\n  </head>\n").unwrap();
        assert!(
            no_rust.contains("    <link data-trunk rel=\"copy-dir\" href=\"assets\" />\n  </head>")
        );
        assert_eq!(with_copy_dir("no head here"), None);
    }

    #[test]
    fn the_ai_hears_what_was_added() {
        assert_eq!(note(&[]), None);
        let n = note(&[AddedFile {
            path: "assets/models/plane.glb".into(),
            kind: AssetKind::Model,
            size: 2048,
        }])
        .unwrap();
        assert!(
            n.contains("- assets/models/plane.glb (3D model, 2 KB)"),
            "{n}"
        );
    }
}
