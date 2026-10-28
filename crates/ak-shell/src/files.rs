//! The project's files (tree), its version history (timeline), and the code
//! editor (CodeMirror through `interop`). Shown in full mode.

use std::collections::{BTreeMap, BTreeSet};

use ak_types::ipc::{
    AssetKind, FileEntry, FileText, Media, NameProblem, RenameFile, Renamed, TimelineRow,
    name_problem,
};
use leptos::prelude::*;

use crate::backend::{Backend, CallError, now};
use crate::i18n::{t, t_count, t_with, tr};
use crate::layout::{CenterTab, Layout};
use crate::shell::change;

/// Editor language, from the file name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Rust,
    Toml,
    Css,
    Html,
    Markdown,
    Plain,
}

impl Lang {
    pub fn of(path: &str) -> Lang {
        match path
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .as_deref()
        {
            Some("rs") => Lang::Rust,
            Some("toml") => Lang::Toml,
            Some("css") => Lang::Css,
            Some("html" | "htm") => Lang::Html,
            Some("md") => Lang::Markdown,
            _ => Lang::Plain,
        }
    }
}

/// A line of the file tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub path: String,
    pub name: String,
    pub depth: usize,
    pub dir: bool,
    /// For folders: shown open.
    pub expanded: bool,
}

/// The visible lines of the tree: folders first, then files, by name; the
/// contents of closed folders are left out.
pub fn visible_rows(entries: &[FileEntry], expanded: &BTreeSet<String>) -> Vec<Row> {
    // parent path ("" = the project) -> children (dir?, name)
    let mut children: BTreeMap<String, Vec<(bool, String)>> = BTreeMap::new();
    let mut dirs: BTreeSet<String> = entries
        .iter()
        .filter(|e| e.dir)
        .map(|e| e.path.clone())
        .collect();
    for e in entries {
        // Folders implied by a file's path count too.
        let mut p = e.path.as_str();
        while let Some((parent, _)) = p.rsplit_once('/') {
            dirs.insert(parent.to_owned());
            p = parent;
        }
    }
    let mut all: BTreeSet<(String, bool)> =
        entries.iter().map(|e| (e.path.clone(), e.dir)).collect();
    for d in &dirs {
        all.insert((d.clone(), true));
    }
    for (path, dir) in &all {
        if *dir && all.contains(&(path.clone(), false)) {
            continue;
        }
        let (parent, name) = path.rsplit_once('/').unwrap_or(("", path));
        children
            .entry(parent.to_owned())
            .or_default()
            .push((*dir, name.to_owned()));
    }
    for list in children.values_mut() {
        list.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase()))
        });
        list.dedup();
    }
    let mut rows = Vec::new();
    fn walk(
        parent: &str,
        depth: usize,
        children: &BTreeMap<String, Vec<(bool, String)>>,
        expanded: &BTreeSet<String>,
        rows: &mut Vec<Row>,
    ) {
        for (dir, name) in children.get(parent).into_iter().flatten() {
            let path = if parent.is_empty() {
                name.clone()
            } else {
                format!("{parent}/{name}")
            };
            let open = *dir && expanded.contains(&path);
            rows.push(Row {
                path: path.clone(),
                name: name.clone(),
                depth,
                dir: *dir,
                expanded: open,
            });
            if open {
                walk(&path, depth + 1, children, expanded, rows);
            }
        }
    }
    walk("", 0, &children, expanded, &mut rows);
    rows
}

/// How long ago, as a fluent id and a number.
pub fn ago(then: u64, now: u64) -> (&'static str, u64) {
    let s = now.saturating_sub(then);
    match s {
        0..60 => ("time-just-now", 0),
        60..3_600 => ("time-minutes-ago", s / 60),
        3_600..86_400 => ("time-hours-ago", s / 3_600),
        _ => ("time-days-ago", s / 86_400),
    }
}

/// The file in the editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenFile {
    pub path: String,
    /// As last read or saved.
    pub saved: String,
    pub binary: bool,
    /// Shown instead of text (a picture, sound, font or 3D model).
    pub media: Option<Media>,
    /// Bumped when the editor must show new text (the file was reloaded).
    pub version: u32,
}

/// The open project's files and history, shared by the panes.
#[derive(Clone, Copy)]
pub struct Files {
    pub entries: RwSignal<Vec<FileEntry>>,
    pub expanded: RwSignal<BTreeSet<String>>,
    pub open: RwSignal<Option<OpenFile>>,
    /// Unsaved edits, per path.
    pub drafts: RwSignal<BTreeMap<String, String>>,
    /// The AI changed the open file while it had unsaved edits.
    pub changed_outside: RwSignal<bool>,
    pub timeline: RwSignal<Vec<TimelineRow>>,
    pub error: RwSignal<Option<String>>,
}

impl Default for Files {
    fn default() -> Self {
        Files::new()
    }
}

fn message(e: CallError) -> String {
    match e {
        CallError::Connect(c) => c.to_string(),
        CallError::Other(d) => d,
    }
}

impl Files {
    pub fn new() -> Files {
        Files {
            entries: RwSignal::new(Vec::new()),
            expanded: RwSignal::new(BTreeSet::from(["src".to_owned()])),
            open: RwSignal::new(None),
            drafts: RwSignal::new(BTreeMap::new()),
            changed_outside: RwSignal::new(false),
            timeline: RwSignal::new(Vec::new()),
            error: RwSignal::new(None),
        }
    }

    /// A project was opened: forget the last one's state.
    pub fn reset(&self) {
        self.entries.set(Vec::new());
        self.open.set(None);
        self.drafts.set(BTreeMap::new());
        self.changed_outside.set(false);
        self.timeline.set(Vec::new());
        self.error.set(None);
    }

    /// Reloads the tree and the timeline, and the open file (after a turn, a
    /// restore, or on open).
    pub fn refresh(self, backend: Backend) {
        leptos::task::spawn_local(async move {
            match backend.list_files().await {
                Ok(list) => self.entries.set(list),
                Err(e) => self.error.set(Some(message(e))),
            }
            if let Ok(rows) = backend.timeline().await {
                self.timeline.set(rows);
            }
            let Some(open) = self.open.get_untracked() else {
                return;
            };
            let Ok(disk) = backend.read_file(&open.path).await else {
                return;
            };
            if disk.text == open.saved && disk.media == open.media {
                return;
            }
            let has_draft = self.drafts.with_untracked(|d| d.contains_key(&open.path));
            if has_draft {
                self.changed_outside.set(true);
            } else {
                self.show(disk, open.version + 1);
            }
        });
    }

    fn show(&self, file: FileText, version: u32) {
        self.changed_outside.set(false);
        self.open.set(Some(OpenFile {
            path: file.path,
            saved: file.text,
            binary: file.binary,
            media: file.media,
            version,
        }));
    }

    pub fn open_file(self, backend: Backend, path: String) {
        leptos::task::spawn_local(async move {
            match backend.read_file(&path).await {
                Ok(file) => {
                    let v = self
                        .open
                        .with_untracked(|o| o.as_ref().map_or(0, |o| o.version + 1));
                    self.show(file, v);
                }
                Err(e) => self.error.set(Some(message(e))),
            }
        });
    }

    /// A file was renamed: its unsaved edits go with it.
    pub fn renamed(&self, from: &str, to: &str) {
        self.drafts.update(|d| {
            if let Some(text) = d.remove(from) {
                d.insert(to.to_owned(), text);
            }
        });
    }

    /// Throws away the open file's edits and shows what is on disk.
    pub fn reload_open(self, backend: Backend) {
        let Some(path) = self
            .open
            .with_untracked(|o| o.as_ref().map(|o| o.path.clone()))
        else {
            return;
        };
        self.drafts.update(|d| {
            d.remove(&path);
        });
        self.open_file(backend, path);
    }

    /// The editor's text changed.
    pub fn edited(&self, path: &str, text: String) {
        let saved = self.open.with_untracked(|o| {
            o.as_ref()
                .filter(|o| o.path == path)
                .map(|o| o.saved.clone())
        });
        self.drafts.update(|d| {
            if saved.as_deref() == Some(text.as_str()) {
                d.remove(path);
            } else {
                d.insert(path.to_owned(), text);
            }
        });
    }

    pub fn save(self, backend: Backend) {
        let Some(path) = self
            .open
            .with_untracked(|o| o.as_ref().map(|o| o.path.clone()))
        else {
            return;
        };
        let Some(text) = self.drafts.with_untracked(|d| d.get(&path).cloned()) else {
            return;
        };
        leptos::task::spawn_local(async move {
            match backend.write_file(&path, &text).await {
                Ok(()) => {
                    self.open.update(|o| {
                        if let Some(o) = o.as_mut().filter(|o| o.path == path) {
                            o.saved = text.clone();
                        }
                    });
                    self.drafts.update(|d| {
                        if d.get(&path) == Some(&text) {
                            d.remove(&path);
                        }
                    });
                    self.changed_outside.set(false);
                }
                Err(e) => self.error.set(Some(message(e))),
            }
        });
    }
}

#[component]
fn Heading(text: &'static str, #[prop(optional)] children: Option<Children>) -> impl IntoView {
    view! {
        <div class="flex items-center justify-between gap-2 px-4 pt-3 pb-2">
            <h2 class="text-xs font-semibold tracking-wide text-gray-500 uppercase dark:text-gray-400">
                {t(text)}
            </h2>
            {children.map(|c| c())}
        </div>
    }
}

#[component]
pub fn FilesPane(layout: RwSignal<Layout>) -> impl IntoView {
    let files = expect_context::<Files>();
    let backend = StoredValue::new(expect_context::<Backend>());
    let rows = move || {
        files
            .entries
            .with(|e| files.expanded.with(|x| visible_rows(e, x)))
    };
    let open_path = move || files.open.with(|o| o.as_ref().map(|o| o.path.clone()));
    view! {
        <section data-pane="file-list" class="flex min-h-0 flex-1 flex-col">
            <Heading text="shell-files">
                <div class="flex gap-1">
                <button
                    title=t("files-open-files-tip")
                    type="button"
                    data-action="open-in-files"
                    class="rounded px-2 py-0.5 text-xs text-blue-700 hover:bg-blue-50 dark:text-blue-300 dark:hover:bg-blue-950"
                    on:click=move |_| {
                        let backend = backend.get_value();
                        leptos::task::spawn_local(async move {
                            let _ = backend.open_in_files(None).await;
                        });
                    }
                >
                    {t("home-open-files")}
                </button>
                <crate::add_files::AddFilesButton small=true />
                </div>
            </Heading>
            <Show
                when=move || files.entries.with(|e| !e.is_empty())
                fallback=|| view! { <p class="px-4 text-sm text-gray-500 dark:text-gray-400">{t("shell-files-empty")}</p> }
            >
                <ul role="tree" class="min-h-0 flex-1 overflow-auto pb-2 text-sm">
                    <For each=rows key=|r| (r.path.clone(), r.expanded) let:row>
                        {
                            let path = row.path.clone();
                            let dirty = {
                                let path = path.clone();
                                move || files.drafts.with(|d| d.contains_key(&path))
                            };
                            let current = {
                                let path = path.clone();
                                move || open_path().as_deref() == Some(path.as_str())
                            };
                            let click = move |_| {
                                if row.dir {
                                    let p = path.clone();
                                    files.expanded.update(|x| {
                                        if !x.remove(&p) {
                                            x.insert(p);
                                        }
                                    });
                                } else {
                                    change(layout, |l| l.tab = CenterTab::Code);
                                    files.open_file(backend.get_value(), path.clone());
                                }
                            };
                            view! {
                                <li role="treeitem" aria-expanded=row.dir.then(|| row.expanded.to_string())>
                                    <button
                                        title=t("files-row-tip")
                                        type="button"
                                        data-file=row.path.clone()
                                        class=move || if current() {
                                            "flex w-full items-center gap-1 bg-blue-50 py-0.5 pr-2 text-left text-blue-900 dark:bg-blue-950 dark:text-blue-100"
                                        } else {
                                            "flex w-full items-center gap-1 py-0.5 pr-2 text-left hover:bg-gray-100 dark:hover:bg-gray-800"
                                        }
                                        style=format!("padding-left:{}px", 12 + row.depth * 14)
                                        on:click=click
                                    >
                                        <span aria-hidden="true" class="w-3 shrink-0 text-xs text-gray-400">
                                            {if row.dir { if row.expanded { "▾" } else { "▸" } } else { "" }}
                                        </span>
                                        <span class=if row.dir { "truncate font-medium" } else { "truncate" }>{row.name.clone()}</span>
                                        <Show when=dirty.clone()>
                                            <span data-dirty="" title=t("files-unsaved") class="ml-auto text-blue-600">"●"</span>
                                        </Show>
                                    </button>
                                </li>
                            }
                        }
                    </For>
                </ul>
            </Show>
        </section>
    }
}

#[component]
pub fn TimelinePane() -> impl IntoView {
    let files = expect_context::<Files>();
    let backend = expect_context::<Backend>();
    let marking = RwSignal::new(false);
    let name = RwSignal::new(String::new());
    let confirming = RwSignal::new(None::<String>);
    let now = now();

    let mark = {
        let backend = backend.clone();
        Callback::new(move |()| {
            let label = name.get_untracked().trim().to_owned();
            if label.is_empty() {
                return;
            }
            let backend = backend.clone();
            leptos::task::spawn_local(async move {
                match backend.mark_version(&label).await {
                    Ok(()) => {
                        marking.set(false);
                        name.set(String::new());
                        files.refresh(backend);
                    }
                    Err(e) => files.error.set(Some(message(e))),
                }
            });
        })
    };
    let restore = Callback::new(move |id: String| {
        confirming.set(None);
        let backend = backend.clone();
        leptos::task::spawn_local(async move {
            match backend.restore(&id).await {
                Ok(()) => files.refresh(backend),
                Err(e) => files.error.set(Some(message(e))),
            }
        });
    });

    view! {
        <section data-pane="timeline" class="flex min-h-0 flex-1 flex-col border-t border-gray-200 dark:border-gray-800">
            <Heading text="shell-timeline">
                <button
                    title=t("timeline-mark-tip")
                    type="button"
                    data-action="mark"
                    class="rounded px-2 py-0.5 text-xs text-blue-700 hover:bg-blue-50 dark:text-blue-300 dark:hover:bg-blue-950"
                    on:click=move |_| marking.update(|m| *m = !*m)
                >
                    {t("timeline-mark")}
                </button>
            </Heading>
            <Show when=move || marking.get()>
                <div class="flex gap-1 px-3 pb-2">
                    <input
                        type="text"
                        placeholder=t("timeline-mark-name")
                        aria-label=t("timeline-mark-name")
                        class="min-w-0 flex-1 rounded border border-gray-300 bg-white px-2 py-1 text-sm dark:border-gray-700 dark:bg-gray-950"
                        prop:value=move || name.get()
                        on:input=move |e| name.set(event_target_value(&e))
                        on:keydown=move |e| if e.key() == "Enter" { mark.run(()) }
                    />
                    <button
                        title=t("timeline-mark-save-tip")
                        type="button"
                        data-action="save-mark"
                        disabled=move || name.with(|n| n.trim().is_empty())
                        class="rounded bg-blue-600 px-2 py-1 text-xs text-white disabled:opacity-50"
                        on:click=move |_| mark.run(())
                    >
                        {t("timeline-mark-save")}
                    </button>
                </div>
            </Show>
            <Show
                when=move || files.timeline.with(|t| !t.is_empty())
                fallback=|| view! { <p class="px-4 text-sm text-gray-500 dark:text-gray-400">{t("shell-timeline-empty")}</p> }
            >
                <ol class="min-h-0 flex-1 space-y-1 overflow-auto px-2 pb-3">
                    <For each=move || files.timeline.get() key=|r| (r.id.clone(), r.current) let:row>
                        {
                            let (when_id, n) = ago(row.created_at, now);
                            let id = row.id.clone();
                            let id2 = row.id.clone();
                            let is_confirming = move || confirming.get().as_deref() == Some(id.as_str());
                            let marked = row.kind == "marked";
                            view! {
                                <li data-entry=row.kind.clone() class=if row.current {
                                    "rounded-lg bg-white p-2 text-sm shadow-sm ring-1 ring-blue-200 dark:bg-gray-950 dark:ring-blue-900"
                                } else {
                                    "rounded-lg p-2 text-sm hover:bg-white dark:hover:bg-gray-950"
                                }>
                                    <div class="flex items-baseline justify-between gap-2">
                                        <span class=if marked { "text-xs font-semibold text-amber-700 dark:text-amber-400" } else { "text-xs text-gray-500 dark:text-gray-400" }>
                                            {tr(format!("timeline-{}", row.kind))}
                                        </span>
                                        <span class="shrink-0 text-xs text-gray-400">{t_count(when_id, "n", n)}</span>
                                    </div>
                                    <p class="mt-0.5 line-clamp-2">{row.label.clone()}</p>
                                    {if row.current {
                                        view! { <p class="mt-1 text-xs text-blue-700 dark:text-blue-300">{t("timeline-current")}</p> }.into_any()
                                    } else {
                                        view! {
                                            <Show
                                                when=is_confirming.clone()
                                                fallback=move || {
                                                    let id = id2.clone();
                                                    view! {
                                                        <button
                                                            title=t("timeline-restore-tip")
                                                            type="button"
                                                            data-action="restore"
                                                            class="mt-1 text-xs text-blue-700 hover:underline dark:text-blue-300"
                                                            on:click=move |_| confirming.set(Some(id.clone()))
                                                        >
                                                            {t("timeline-restore-button")}
                                                        </button>
                                                    }
                                                }
                                            >
                                                {
                                                    let id = row.id.clone();
                                                    view! {
                                                        <div data-confirm="restore" class="mt-1 rounded bg-amber-50 p-2 text-xs text-amber-900 dark:bg-amber-950 dark:text-amber-100">
                                                            <p>{t("timeline-restore-confirm")}</p>
                                                            <div class="mt-1 flex gap-2">
                                                                <button title=t("timeline-restore-yes-tip") type="button" data-action="confirm-restore" class="rounded bg-blue-600 px-2 py-0.5 text-white" on:click=move |_| restore.run(id.clone())>
                                                                    {t("timeline-restore-yes")}
                                                                </button>
                                                                <button title=t("common-cancel-tip") type="button" class="rounded px-2 py-0.5 hover:bg-amber-100 dark:hover:bg-amber-900" on:click=move |_| confirming.set(None)>
                                                                    {t("common-cancel")}
                                                                </button>
                                                            </div>
                                                        </div>
                                                    }
                                                }
                                            </Show>
                                        }.into_any()
                                    }}
                                </li>
                            }
                        }
                    </For>
                </ol>
            </Show>
        </section>
    }
}

#[component]
pub fn CodePane() -> impl IntoView {
    let files = expect_context::<Files>();
    let backend = expect_context::<Backend>();
    let host = NodeRef::<leptos::html::Div>::new();
    let path = move || files.open.with(|o| o.as_ref().map(|o| o.path.clone()));
    let dirty = move || path().is_some_and(|p| files.drafts.with(|d| d.contains_key(&p)));
    let binary = move || files.open.with(|o| o.as_ref().is_some_and(|o| o.binary));

    #[cfg(target_arch = "wasm32")]
    editor::mount(files, backend.clone(), host);

    let backend = StoredValue::new(backend);
    view! {
        <div data-pane="code" class="flex min-h-0 flex-1 flex-col bg-white dark:bg-gray-950">
            {move || files.error.get().map(|e| view! {
                <div role="alert" class="flex items-start justify-between gap-2 border-b border-red-200 bg-red-50 px-3 py-2 text-sm text-red-900 dark:border-red-900 dark:bg-red-950 dark:text-red-100">
                    <div>
                        <p>{t("files-error")}</p>
                        <details class="text-xs"><summary class="cursor-pointer">{t("common-details")}</summary><pre class="whitespace-pre-wrap">{e}</pre></details>
                    </div>
                    <button title=t("common-ok-tip") type="button" class="text-xs underline" on:click=move |_| files.error.set(None)>{t("common-ok")}</button>
                </div>
            })}
            <Show
                when=move || path().is_some()
                fallback=|| view! {
                    <div class="flex flex-1 items-center justify-center p-6 text-sm text-gray-500 dark:text-gray-400">{t("shell-code-empty")}</div>
                }
            >
                <div class="flex shrink-0 items-center gap-2 border-b border-gray-200 px-3 py-1.5 text-sm dark:border-gray-800">
                    <RenameFile />
                    <Show when=dirty>
                        <span data-dirty="" class="text-xs text-blue-700 dark:text-blue-300">{t("files-unsaved")}</span>
                    </Show>
                    <span class="flex-1"></span>
                    <button
                        type="button"
                        data-action="save"
                        title=t("files-save-hint")
                        disabled=move || !dirty()
                        class="rounded bg-blue-600 px-3 py-1 text-xs font-medium text-white hover:bg-blue-700 disabled:opacity-40"
                        on:click=move |_| files.save(backend.get_value())
                    >
                        {t("files-save")}
                    </button>
                </div>
                <Show when=move || files.changed_outside.get()>
                    <div data-notice="changed-outside" class="flex flex-wrap items-center gap-2 border-b border-amber-200 bg-amber-50 px-3 py-2 text-sm text-amber-900 dark:border-amber-900 dark:bg-amber-950 dark:text-amber-100">
                        <span class="flex-1">{t("files-changed-outside")}</span>
                        <button
                            title=t("files-reload-tip")
                            type="button"
                            data-action="reload-file"
                            class="rounded bg-amber-600 px-2 py-0.5 text-xs text-white"
                            on:click=move |_| files.reload_open(backend.get_value())
                        >
                            {t("files-reload")}
                        </button>
                        <button title=t("files-keep-mine-tip") type="button" class="text-xs underline" on:click=move |_| files.changed_outside.set(false)>
                            {t("files-keep-mine")}
                        </button>
                    </div>
                </Show>
                <Show when=binary>
                    {move || match files.open.with(|o| o.as_ref().and_then(|o| o.media.clone())) {
                        Some(media) => view! { <MediaView media /> }.into_any(),
                        None => view! {
                            <p class="p-6 text-sm text-gray-500 dark:text-gray-400">{t("files-binary")}</p>
                        }.into_any(),
                    }}
                </Show>
            </Show>
            <div node_ref=host data-editor="" class=move || if path().is_some() && !binary() { "min-h-0 flex-1 overflow-auto" } else { "hidden" }></div>
        </div>
    }
}

/// Why a rename did not happen (translated when drawn).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenameTrouble {
    Name(NameProblem),
    Taken,
    Failed(String),
}

fn trouble_text(trouble: &RenameTrouble) -> String {
    match trouble {
        RenameTrouble::Name(NameProblem::Empty) => t("files-rename-empty")(),
        RenameTrouble::Name(NameProblem::Character { found }) => {
            t_with("files-rename-character", [("found", found.clone())])()
        }
        RenameTrouble::Name(NameProblem::Ending) => t("files-rename-ending")(),
        RenameTrouble::Name(NameProblem::Reserved) => t("files-rename-reserved")(),
        RenameTrouble::Name(NameProblem::TooLong) => t("files-rename-too-long")(),
        RenameTrouble::Taken => t("files-rename-taken")(),
        RenameTrouble::Failed(e) => t_with("files-rename-failed", [("error", e.clone())])(),
    }
}

/// `assets/images/cat.png` → (`cat`, `.png`).
pub fn stem_and_extension(path: &str) -> (String, String) {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem.to_owned(), format!(".{ext}")),
        _ => (name.to_owned(), String::new()),
    }
}

/// The open file's name, and for the user's files in `assets/` a way to
/// rename them (same folder and extension; the project's text that names the
/// file follows).
#[component]
fn RenameFile() -> impl IntoView {
    let files = expect_context::<Files>();
    let backend = StoredValue::new(expect_context::<Backend>());
    let path = move || {
        files
            .open
            .with(|o| o.as_ref().map(|o| o.path.clone()))
            .unwrap_or_default()
    };
    // The new name being typed (without the extension).
    let editing = RwSignal::new(None::<String>);
    let trouble = RwSignal::new(None::<RenameTrouble>);
    // After a rename: the new path and how many files named it.
    let updated = RwSignal::new(None::<(String, usize)>);
    let input = NodeRef::<leptos::html::Input>::new();

    #[cfg(target_arch = "wasm32")]
    Effect::new(move |_| {
        if let Some(el) = input.get() {
            let _ = el.focus();
            el.select();
        }
    });

    let start = move || {
        trouble.set(None);
        updated.set(None);
        editing.set(Some(stem_and_extension(&path()).0));
    };
    let cancel = move || {
        editing.set(None);
        trouble.set(None);
    };
    let submit = move || {
        let Some(stem) = editing.get_untracked() else {
            return;
        };
        let stem = stem.trim().to_owned();
        if let Some(problem) = name_problem(&stem) {
            trouble.set(Some(RenameTrouble::Name(problem)));
            return;
        }
        let from = files
            .open
            .with_untracked(|o| o.as_ref().map(|o| o.path.clone()))
            .unwrap_or_default();
        let backend = backend.get_value();
        leptos::task::spawn_local(async move {
            let request = RenameFile {
                path: from.clone(),
                stem,
            };
            match backend.rename_file(&request).await {
                Ok(Renamed::Done { path, updated: u }) => {
                    editing.set(None);
                    trouble.set(None);
                    files.renamed(&from, &path);
                    if !u.is_empty() {
                        updated.set(Some((path.clone(), u.len())));
                    }
                    files.open_file(backend.clone(), path);
                    files.refresh(backend);
                }
                Ok(Renamed::Taken) => trouble.set(Some(RenameTrouble::Taken)),
                Ok(Renamed::Bad { problem }) => trouble.set(Some(RenameTrouble::Name(problem))),
                Err(e) => trouble.set(Some(RenameTrouble::Failed(message(e)))),
            }
        });
    };
    let can_rename = move || path().starts_with("assets/");

    view! {
        <Show
            when=move || editing.with(Option::is_some)
            fallback=move || view! {
                <span data-open-file="" class="truncate font-mono text-xs">{path}</span>
                <Show when=can_rename>
                    <button
                        title=t("files-rename-tip")
                        type="button"
                        data-action="rename-file"
                        class="shrink-0 rounded px-2 py-0.5 text-xs whitespace-nowrap text-blue-700 hover:bg-blue-50 dark:text-blue-300 dark:hover:bg-blue-950"
                        on:click=move |_| start()
                    >
                        {t("files-rename")}
                    </button>
                </Show>
                {move || updated.get().filter(|(p, _)| *p == path()).map(|(_, n)| view! {
                    <span data-renamed="" class="truncate text-xs text-green-700 dark:text-green-300">
                        {t_count("files-renamed-updated", "count", n as u64)}
                    </span>
                })}
            }
        >
            <div data-rename="" class="flex min-w-0 flex-1 flex-wrap items-center gap-1">
                <span class="truncate font-mono text-xs text-gray-500 dark:text-gray-400">
                    {move || path().rsplit_once('/').map(|(dir, _)| format!("{dir}/")).unwrap_or_default()}
                </span>
                <input
                    node_ref=input
                    type="text"
                    aria-label=t("files-rename-label")
                    maxlength="120"
                    class="w-40 min-w-0 rounded border border-gray-300 bg-white px-2 py-0.5 font-mono text-xs dark:border-gray-700 dark:bg-gray-950"
                    prop:value=move || editing.get().unwrap_or_default()
                    on:input=move |e| {
                        trouble.set(None);
                        editing.set(Some(event_target_value(&e)));
                    }
                    on:keydown=move |e| match e.key().as_str() {
                        "Enter" => submit(),
                        "Escape" => cancel(),
                        _ => {}
                    }
                />
                <span class="font-mono text-xs text-gray-500 dark:text-gray-400">{move || stem_and_extension(&path()).1}</span>
                <button
                    title=t("files-rename-save-tip")
                    type="button"
                    data-action="save-rename"
                    class="rounded bg-blue-600 px-2 py-0.5 text-xs font-medium text-white hover:bg-blue-700"
                    on:click=move |_| submit()
                >
                    {t("files-rename-save")}
                </button>
                <button
                    title=t("common-cancel-tip")
                    type="button"
                    class="rounded px-2 py-0.5 text-xs hover:bg-gray-100 dark:hover:bg-gray-800"
                    on:click=move |_| cancel()
                >
                    {t("common-cancel")}
                </button>
                {move || trouble.get().map(|tr| view! {
                    <span role="alert" data-rename-trouble="" class="w-full text-xs text-red-700 dark:text-red-300">{trouble_text(&tr)}</span>
                })}
            </div>
        </Show>
    }
}

/// `1.5 MB`, `320 KB`, `12 B`.
pub fn size_text(bytes: u64) -> String {
    const K: f64 = 1024.0;
    let b = bytes as f64;
    if b < K {
        format!("{bytes} B")
    } else if b < K * K {
        format!("{:.0} KB", b / K)
    } else {
        format!("{:.1} MB", b / (K * K))
    }
}

/// Model files the preview can show (others are parts of one: `.bin`, `.mtl`).
const SHOWN_MODELS: &[&str] = &["glb", "gltf", "obj", "fbx"];

/// The extension of a URL's file, lowercase (`…/plane.GLB?v=1` → `glb`).
pub fn url_extension(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// What the model view says: `loading 40`, `ready` or `failed: …` from the JS.
fn model_state_text(state: &str) -> String {
    if let Some(p) = state.strip_prefix("loading ") {
        t_with("files-model-loading", [("percent", p.to_owned())])()
    } else if let Some(e) = state.strip_prefix("failed: ") {
        t_with("files-model-failed", [("error", e.to_owned())])()
    } else {
        t("files-model-ready")()
    }
}

/// A 3D model, turning; drag to turn it, scroll to zoom.
#[component]
fn ModelPreview(url: String, size: String) -> impl IntoView {
    let state = RwSignal::new("loading 0".to_owned());
    let canvas = NodeRef::<leptos::html::Canvas>::new();
    #[cfg(target_arch = "wasm32")]
    {
        let view = StoredValue::new_local(
            None::<(
                crate::interop::ModelView,
                wasm_bindgen::closure::Closure<dyn FnMut(String)>,
            )>,
        );
        let ext = url_extension(&url);
        Effect::new(move |_| {
            let Some(el) = canvas.get() else {
                return;
            };
            if view.with_value(Option::is_some) {
                return;
            }
            let on_state =
                wasm_bindgen::closure::Closure::<dyn FnMut(String)>::new(move |s: String| {
                    state.set(s)
                });
            let v = crate::interop::ModelView::new(&el, &url, &ext, &on_state);
            view.set_value(Some((v, on_state)));
        });
        on_cleanup(move || {
            if let Some((v, _)) = view.try_update_value(Option::take).flatten() {
                v.dispose();
            }
        });
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = url;
    view! {
        <div data-media="model" class="flex min-h-0 flex-1 flex-col">
            <canvas node_ref=canvas class="min-h-0 w-full flex-1 cursor-grab touch-none"></canvas>
            <p data-model-state=move || state.get() class="px-4 py-2 text-xs text-gray-500 dark:text-gray-400">
                {move || model_state_text(&state.get())}" · "{size}
            </p>
        </div>
    }
}

/// A picture, sound, font or 3D model shown as itself.
#[component]
fn MediaView(media: Media) -> impl IntoView {
    let size = size_text(media.size);
    let url = media.url;
    match media.kind {
        AssetKind::Image => {
            let dims = RwSignal::new(None::<(u32, u32)>);
            #[cfg(target_arch = "wasm32")]
            let on_load = move |ev: leptos::ev::Event| {
                use wasm_bindgen::JsCast;
                if let Some(img) = ev
                    .target()
                    .and_then(|t| t.dyn_into::<web_sys::HtmlImageElement>().ok())
                {
                    dims.set(Some((img.natural_width(), img.natural_height())));
                }
            };
            #[cfg(not(target_arch = "wasm32"))]
            let on_load = move |_: leptos::ev::Event| {};
            view! {
                <div data-media="image" class="flex min-h-0 flex-1 flex-col items-center gap-2 overflow-auto p-4">
                    <img
                        src=url
                        alt=""
                        on:load=on_load
                        class="max-w-full rounded border border-gray-200 bg-[repeating-conic-gradient(#e5e7eb_0_25%,#fff_0_50%)] bg-[length:16px_16px] dark:border-gray-700"
                    />
                    <p class="text-xs text-gray-500 dark:text-gray-400">
                        {move || match dims.get() {
                            Some((w, h)) => format!("{w} × {h} · {size}"),
                            None => size.clone(),
                        }}
                    </p>
                </div>
            }
            .into_any()
        }
        AssetKind::Sound => view! {
            <div data-media="sound" class="flex flex-col items-center gap-3 p-6">
                <audio controls src=url class="w-full max-w-md"></audio>
                <p class="text-xs text-gray-500 dark:text-gray-400">{size}</p>
            </div>
        }
        .into_any(),
        AssetKind::Font => {
            // One family per file version, so a changed font shows anew.
            let family: String = url.chars().filter(char::is_ascii_alphanumeric).collect();
            let face = format!(r#"@font-face{{font-family:"{family}";src:url("{url}")}}"#);
            let style = format!(r#"font-family:"{family}",sans-serif"#);
            view! {
                <div data-media="font" class="flex min-h-0 flex-1 flex-col gap-3 overflow-auto p-6 text-gray-900 dark:text-gray-100">
                    <style>{face}</style>
                    <p style=style.clone() class="text-4xl">{t("files-font-sample")}</p>
                    <p style=style.clone() class="text-2xl">{t("files-font-upper")}</p>
                    <p style=style.clone() class="text-2xl">{t("files-font-lower")}</p>
                    <p style=style class="text-base">{t("files-font-sample")}</p>
                    <p class="text-xs text-gray-500 dark:text-gray-400">{size}</p>
                </div>
            }
            .into_any()
        }
        AssetKind::Model if SHOWN_MODELS.contains(&url_extension(&url).as_str()) => {
            view! { <ModelPreview url size /> }.into_any()
        }
        AssetKind::Model | AssetKind::Other => view! {
            <div data-media="model-part" class="p-6 text-sm text-gray-500 dark:text-gray-400">
                <p>{t("files-model-part")}</p>
                <p class="mt-2 text-xs">{size}</p>
            </div>
        }
        .into_any(),
    }
}

#[cfg(target_arch = "wasm32")]
mod editor {
    //! Keeps one CodeMirror view in step with the open file.

    use leptos::prelude::*;

    use super::{Files, Lang};
    use crate::backend::Backend;
    use crate::interop::Editor;

    pub fn mount(files: Files, backend: Backend, host: NodeRef<leptos::html::Div>) {
        let current = StoredValue::new_local(None::<Editor>);
        // Only a new file or a reload makes a fresh view (not saving or typing).
        let shown = Memo::new(move |_| {
            files
                .open
                .with(|o| o.as_ref().map(|o| (o.path.clone(), o.version, o.binary)))
        });
        Effect::new(move |_| {
            let open = shown.get();
            current.set_value(None);
            let (Some((path, _, false)), Some(el)) = (open, host.get()) else {
                return;
            };
            let text = files
                .drafts
                .with_untracked(|d| d.get(&path).cloned())
                .or_else(|| {
                    files
                        .open
                        .with_untracked(|o| o.as_ref().map(|o| o.saved.clone()))
                })
                .unwrap_or_default();
            let dark = web_sys::window()
                .and_then(|w| w.match_media("(prefers-color-scheme: dark)").ok().flatten())
                .is_some_and(|m| m.matches());
            let p = path.clone();
            let b = backend.clone();
            let editor = Editor::new(
                &el,
                &text,
                Lang::of(&path),
                dark,
                move |text| files.edited(&p, text),
                move || files.save(b.clone()),
            );
            if let Ok(e) = editor {
                e.focus();
                current.set_value(Some(e));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, dir: bool) -> FileEntry {
        FileEntry {
            path: path.into(),
            dir,
        }
    }

    #[test]
    fn the_tree_lists_folders_first_and_hides_closed_ones() {
        let entries = vec![
            entry("Cargo.toml", false),
            entry("README.md", false),
            entry("src", true),
            entry("src/main.rs", false),
            entry("src/screens/home.rs", false),
            entry("src-tauri/src/main.rs", false),
        ];
        let closed = visible_rows(&entries, &BTreeSet::new());
        let names: Vec<_> = closed.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["src", "src-tauri", "Cargo.toml", "README.md"]);
        assert!(closed[0].dir && !closed[0].expanded);

        let open = visible_rows(
            &entries,
            &BTreeSet::from(["src".to_owned(), "src/screens".to_owned()]),
        );
        let paths: Vec<_> = open.iter().map(|r| (r.path.as_str(), r.depth)).collect();
        assert_eq!(
            paths,
            [
                ("src", 0),
                ("src/screens", 1),
                ("src/screens/home.rs", 2),
                ("src/main.rs", 1),
                ("src-tauri", 0),
                ("Cargo.toml", 0),
                ("README.md", 0),
            ]
        );
    }

    #[test]
    fn languages_and_times() {
        assert_eq!(Lang::of("src/main.rs"), Lang::Rust);
        assert_eq!(Lang::of("Cargo.TOML"), Lang::Toml);
        assert_eq!(Lang::of("style/input.css"), Lang::Css);
        assert_eq!(Lang::of("index.html"), Lang::Html);
        assert_eq!(Lang::of("README.md"), Lang::Markdown);
        assert_eq!(Lang::of("gitignore"), Lang::Plain);
        assert_eq!(ago(100, 130), ("time-just-now", 0));
        assert_eq!(ago(0, 125), ("time-minutes-ago", 2));
        assert_eq!(ago(0, 7_300), ("time-hours-ago", 2));
        assert_eq!(ago(0, 3 * 86_400), ("time-days-ago", 3));
        assert_eq!(ago(500, 100), ("time-just-now", 0), "clock moved back");
    }
}
