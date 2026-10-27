//! The user's own files into the project (P1-SHELL-007): "Add files…" (in
//! the chat bar, so simple mode has it, and in the Files pane) and dropping
//! files anywhere on the workspace. Large files and unknown kinds wait for
//! a yes; the AI hears of the added files with the next message.

use ak_types::ipc::{AddFiles, AddedFiles, HeldFile, HoldReason, NewFile};
use leptos::prelude::*;

use crate::backend::{Backend, CallError};
use crate::i18n::{t, t_count, t_with};

/// Dropped files are read by the page and sent whole: larger ones go
/// through "Add files…", which copies from the disk.
pub const DROP_MAX: f64 = 50.0 * 1024.0 * 1024.0;

/// Something to tell the user (shown in their language when drawn).
#[derive(Debug, Clone, PartialEq)]
pub enum Notice {
    Added(usize),
    Failed { name: String, error: String },
    TooLargeToDrop { name: String, bytes: u64 },
    Error(String),
}

/// What adding files shows, shared by the buttons, the drop target and the
/// notice above the chat input (provided by the workspace).
#[derive(Clone, Copy)]
pub struct AddFilesUi {
    pub busy: RwSignal<bool>,
    /// Files dragged over the workspace right now.
    pub dragging: RwSignal<bool>,
    /// Waiting for a yes: the files to send again, and why.
    pub held: RwSignal<Option<(Vec<NewFile>, Vec<HeldFile>)>>,
    pub notice: RwSignal<Vec<Notice>>,
}

impl AddFilesUi {
    pub fn new() -> AddFilesUi {
        AddFilesUi {
            busy: RwSignal::new(false),
            dragging: RwSignal::new(false),
            held: RwSignal::new(None),
            notice: RwSignal::new(Vec::new()),
        }
    }

    /// The workspace's, or a new one (a pane shown on its own).
    pub fn get() -> AddFilesUi {
        use_context::<AddFilesUi>().unwrap_or_else(|| {
            let ui = AddFilesUi::new();
            provide_context(ui);
            ui
        })
    }
}

impl Default for AddFilesUi {
    fn default() -> Self {
        AddFilesUi::new()
    }
}

fn megabytes(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / (1024.0 * 1024.0))
}

/// What to tell the user about a result: how many were added, what failed.
pub fn notices(r: &AddedFiles) -> Vec<Notice> {
    let mut out = Vec::new();
    if !r.added.is_empty() {
        out.push(Notice::Added(r.added.len()));
    }
    out.extend(r.failed.iter().map(|f| Notice::Failed {
        name: f.name.clone(),
        error: f.error.clone(),
    }));
    out
}

/// Sends `files` (`allow`: the user agreed to the held ones) and shows the
/// result; held files wait for a yes.
pub fn submit(backend: Backend, ui: AddFilesUi, files: Vec<NewFile>, allow: bool) {
    if files.is_empty() {
        return;
    }
    ui.busy.set(true);
    ui.held.set(None);
    leptos::task::spawn_local(async move {
        let request = AddFiles {
            files: files.clone(),
            allow,
        };
        match backend.add_files(&request).await {
            Ok(r) => {
                let again: Vec<NewFile> = r
                    .held
                    .iter()
                    .filter_map(|h| files.get(h.index).cloned())
                    .collect();
                ui.held
                    .set((!r.held.is_empty()).then(|| (again, r.held.clone())));
                // The Files pane reloads on the app's PROJECT_CHANGED_EVENT.
                ui.notice.update(|n| n.extend(notices(&r)));
            }
            Err(e) => ui.notice.update(|n| {
                n.push(Notice::Error(match e {
                    CallError::Connect(c) => c.to_string(),
                    CallError::Other(d) => d,
                }))
            }),
        }
        ui.busy.set(false);
    });
}

/// "Add files…": the system's file dialog, then `submit`.
#[component]
pub fn AddFilesButton(#[prop(optional)] small: bool) -> impl IntoView {
    let ui = AddFilesUi::get();
    let backend = StoredValue::new(expect_context::<Backend>());
    let pick = move |_| {
        let backend = backend.get_value();
        leptos::task::spawn_local(async move {
            if let Ok(Some(paths)) = backend.pick_files().await {
                let picked = paths
                    .into_iter()
                    .map(|path| NewFile::Path { path })
                    .collect();
                submit(backend, ui, picked, false);
            }
        });
    };
    let class = if small {
        "rounded px-2 py-0.5 text-xs text-blue-700 hover:bg-blue-50 disabled:opacity-50 dark:text-blue-300 dark:hover:bg-blue-950"
    } else {
        "rounded-lg border border-gray-300 px-3 py-2 text-sm hover:bg-gray-100 disabled:opacity-50 dark:border-gray-700 dark:hover:bg-gray-800"
    };
    let adding = t("files-adding");
    let add = t("files-add");
    view! {
        <button
            title=t("files-add-tip")
            type="button"
            data-action="add-files"
            class=class
            disabled=move || ui.busy.get()
            on:click=pick
        >
            {move || if ui.busy.get() { adding() } else { add() }}
        </button>
    }
}

/// One notice in the user's language.
fn notice_text(n: &Notice) -> String {
    match n {
        Notice::Added(count) => t_count("files-added", "count", *count as u64)(),
        Notice::Failed { name, error } => t_with(
            "files-failed",
            [("name", name.clone()), ("error", error.clone())],
        )(),
        Notice::TooLargeToDrop { name, bytes } => t_with(
            "files-drop-too-large",
            [("name", name.clone()), ("size", megabytes(*bytes))],
        )(),
        Notice::Error(text) => text.clone(),
    }
}

fn held_text(h: &HeldFile) -> String {
    match h.reason {
        HoldReason::Large => t_with(
            "files-held-large",
            [("name", h.name.clone()), ("size", megabytes(h.size))],
        )(),
        HoldReason::Unknown => t_with("files-held-unknown", [("name", h.name.clone())])(),
    }
}

/// Above the chat input: files waiting for a yes, and what happened.
#[component]
pub fn AddFilesNotice() -> impl IntoView {
    let ui = AddFilesUi::get();
    let backend = StoredValue::new(expect_context::<Backend>());
    view! {
        {move || ui.held.get().map(|(_, held)| view! {
            <div data-notice="held" class="mb-2 rounded-lg border border-amber-300 bg-amber-50 px-3 py-2 text-xs text-amber-900 dark:border-amber-800 dark:bg-amber-950 dark:text-amber-100">
                <p class="font-medium">{t("files-held-title")}</p>
                <ul class="mt-1 list-disc pl-4">
                    {held.iter().map(|h| view! { <li>{held_text(h)}</li> }).collect_view()}
                </ul>
                <div class="mt-2 flex gap-2">
                    <button
                        title=t("files-held-add-tip")
                        type="button"
                        data-action="add-held"
                        class="rounded bg-amber-600 px-2 py-1 font-medium text-white hover:bg-amber-700"
                        on:click=move |_| {
                            if let Some((files, _)) = ui.held.get_untracked() {
                                submit(backend.get_value(), ui, files, true);
                            }
                        }
                    >
                        {t("files-held-add")}
                    </button>
                    <button
                        title=t("files-held-skip-tip")
                        type="button"
                        class="rounded px-2 py-1 hover:bg-amber-100 dark:hover:bg-amber-900"
                        on:click=move |_| ui.held.set(None)
                    >
                        {t("files-held-skip")}
                    </button>
                </div>
            </div>
        })}
        {move || {
            let lines = ui.notice.get();
            (!lines.is_empty()).then(|| view! {
                <div data-notice="files" class="mb-2 flex items-start gap-2 rounded-lg bg-gray-100 px-3 py-2 text-xs text-gray-800 dark:bg-gray-800 dark:text-gray-100">
                    <div class="min-w-0 flex-1">
                        {lines.iter().map(|n| view! { <p>{notice_text(n)}</p> }).collect_view()}
                    </div>
                    <button
                        type="button"
                        aria-label=t("files-notice-close")
                        title=t("files-notice-close")
                        class="rounded px-1 hover:bg-gray-200 dark:hover:bg-gray-700"
                        on:click=move |_| ui.notice.set(Vec::new())
                    >
                        "×"
                    </button>
                </div>
            })
        }}
    }
}

/// Over the whole workspace while files are dragged over it: it takes the
/// drop (also above the preview, whose page would get it otherwise).
#[component]
pub fn DropOverlay() -> impl IntoView {
    let ui = AddFilesUi::get();
    view! {
        <Show when=move || ui.dragging.get()>
            <div
                data-drop="files"
                class="absolute inset-0 z-40 flex items-center justify-center border-4 border-dashed border-blue-500 bg-blue-50/80 text-lg font-medium text-blue-900 dark:bg-blue-950/80 dark:text-blue-100"
            >
                {t("files-drop")}
            </div>
        </Show>
    }
}

/// Base64 (standard alphabet, padded), for dropped files' contents.
pub fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(char::from(ABC[((n >> shift) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(target_arch = "wasm32")]
pub mod browser {
    //! Dropping files: the page reads them (no path is given) and sends them.

    use ak_types::ipc::NewFile;
    use leptos::prelude::Update;

    use super::{AddFilesUi, DROP_MAX, Notice, base64, submit};
    use crate::backend::Backend;

    /// Whether a drag carries files from outside (not text or the page's own).
    pub fn has_files(e: &web_sys::DragEvent) -> bool {
        e.data_transfer()
            .is_some_and(|d| d.types().includes(&"Files".into(), 0))
    }

    async fn read(file: web_sys::File) -> Result<NewFile, String> {
        let buf = wasm_bindgen_futures::JsFuture::from(file.array_buffer())
            .await
            .map_err(|_| "could not be read".to_owned())?;
        let bytes = js_sys::Uint8Array::new(&buf).to_vec();
        Ok(NewFile::Dropped {
            name: file.name(),
            data_base64: base64(&bytes),
        })
    }

    /// Files dropped on the workspace: read, then sent like picked ones.
    pub fn dropped(e: &web_sys::DragEvent, backend: Backend, ui: AddFilesUi) {
        let Some(list) = e.data_transfer().and_then(|d| d.files()) else {
            return;
        };
        let files: Vec<web_sys::File> = (0..list.length()).filter_map(|i| list.get(i)).collect();
        leptos::task::spawn_local(async move {
            let mut out = Vec::new();
            let mut notes = Vec::new();
            for file in files {
                let name = file.name();
                if file.size() > DROP_MAX {
                    notes.push(Notice::TooLargeToDrop {
                        name,
                        bytes: file.size() as u64,
                    });
                    continue;
                }
                match read(file).await {
                    Ok(f) => out.push(f),
                    Err(error) => notes.push(Notice::Failed { name, error }),
                }
            }
            if !notes.is_empty() {
                ui.notice.update(|n| n.extend(notes));
            }
            submit(backend, ui, out, false);
        });
    }

    /// Files dropped where nothing takes them must not replace the page.
    pub fn guard_window() {
        let stop = |e: web_sys::DragEvent| {
            if has_files(&e) {
                e.prevent_default();
            }
        };
        let over = leptos::prelude::window_event_listener(leptos::ev::dragover, stop);
        let drop = leptos::prelude::window_event_listener(leptos::ev::drop, stop);
        // For the window's lifetime.
        std::mem::forget((over, drop));
    }
}

#[cfg(test)]
mod tests {
    use ak_types::ipc::{AddedFile, AssetKind, FailedFile};

    use super::*;

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(&[0xff, 0xfe, 0x00]), "//4A");
    }

    #[test]
    fn a_result_says_how_many_and_what_failed() {
        let r = AddedFiles {
            added: vec![AddedFile {
                path: "assets/images/a.png".into(),
                kind: AssetKind::Image,
                size: 1,
            }],
            held: Vec::new(),
            failed: vec![FailedFile {
                name: "b.png".into(),
                error: "gone".into(),
            }],
        };
        assert_eq!(
            notices(&r),
            [
                Notice::Added(1),
                Notice::Failed {
                    name: "b.png".into(),
                    error: "gone".into()
                }
            ]
        );
        assert!(notices(&AddedFiles::default()).is_empty());
    }
}
