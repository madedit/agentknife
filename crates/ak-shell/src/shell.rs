//! A project's workspace: three panes (files + timeline | preview and code |
//! chat). Simple mode hides the files pane and the code tab.

use leptos::prelude::*;

use crate::add_files::{AddFilesUi, DropOverlay};
use crate::chat::ChatPane;
use crate::files::{CodePane, FilesPane, TimelinePane};
use crate::i18n::{t, tr};
use crate::layout::{self, CHAT, CenterTab, LEFT, Layout, Mode};
use crate::logs::LogPane;
use crate::preview::PreviewPane;
use crate::splitter::{Grows, Splitter};

#[component]
pub fn Workspace(layout: RwSignal<Layout>) -> impl IntoView {
    let full = move || layout.with(|l| l.mode == Mode::Full);
    // Files added from anywhere in the workspace (buttons, dropping).
    let adding = AddFilesUi::new();
    provide_context(adding);
    #[cfg(target_arch = "wasm32")]
    let backend = StoredValue::new(expect_context::<crate::backend::Backend>());

    view! {
        <div
            data-screen="workspace"
            class="relative flex min-h-0 flex-1"
            on:dragover=move |_e| {
                #[cfg(target_arch = "wasm32")]
                if crate::add_files::browser::has_files(&_e) {
                    _e.prevent_default();
                    adding.dragging.set(true);
                }
            }
            on:dragleave=move |_e| {
                // Only when leaving the workspace, not moving between its parts.
                #[cfg(target_arch = "wasm32")]
                if _e.related_target().is_none() {
                    adding.dragging.set(false);
                }
            }
            on:drop=move |_e| {
                adding.dragging.set(false);
                #[cfg(target_arch = "wasm32")]
                if crate::add_files::browser::has_files(&_e) {
                    _e.prevent_default();
                    crate::add_files::browser::dropped(&_e, backend.get_value(), adding);
                }
            }
        >
                <DropOverlay />
                <Show when=full>
                    <aside
                        data-pane="files"
                        class="flex shrink-0 flex-col bg-gray-50 dark:bg-gray-900"
                        style:width=move || format!("min({}px, 30vw)", layout.with(|l| l.left))
                    >
                        <FilesPane layout />
                        <TimelinePane />
                    </aside>
                    <Splitter
                        label=Signal::derive(t("shell-resize-files"))
                        width=Signal::derive(move || layout.with(|l| l.left))
                        bounds=LEFT
                        grows=Grows::Right
                        set_width=Callback::new(move |w| change(layout, |l| l.left = w))
                    />
                </Show>
                <main data-pane="center" class="flex min-w-0 flex-1 flex-col bg-gray-100 dark:bg-gray-900/60">
                    <Center layout />
                </main>
                <Splitter
                    label=Signal::derive(t("shell-resize-chat"))
                    width=Signal::derive(move || layout.with(|l| l.chat))
                    bounds=CHAT
                    grows=Grows::Left
                    set_width=Callback::new(move |w| change(layout, |l| l.chat = w))
                />
                <aside
                    data-pane="chat"
                    class="flex shrink-0 flex-col"
                    style:width=move || format!("min({}px, 45vw)", layout.with(|l| l.chat))
                >
                    <ChatPane />
                </aside>
        </div>
    }
}

/// Changes the layout and saves it.
pub fn change(layout: RwSignal<Layout>, f: impl FnOnce(&mut Layout)) {
    layout.update(f);
    layout.with_untracked(layout::save);
}

/// The preview, plus the code and log tabs in full mode.
#[component]
fn Center(layout: RwSignal<Layout>) -> impl IntoView {
    let full = move || layout.with(|l| l.mode == Mode::Full);
    // Memoized: other layout changes (a splitter drag) must not rebuild the pane.
    let current = Memo::new(move |_| {
        if full() {
            layout.with(|l| l.tab)
        } else {
            CenterTab::Preview
        }
    });
    let tab = move |which: CenterTab, text: &'static str| {
        let on = move || layout.with(|l| l.tab == which);
        view! {
            <button
                title=tr(format!("{text}-tip"))
                type="button"
                role="tab"
                aria-selected=move || on().to_string()
                class=move || {
                    if on() {
                        "border-b-2 border-blue-600 px-3 py-2 text-sm font-medium text-gray-900 dark:text-white"
                    } else {
                        "border-b-2 border-transparent px-3 py-2 text-sm text-gray-600 hover:text-gray-900 dark:text-gray-400 dark:hover:text-white"
                    }
                }
                on:click=move |_| change(layout, |l| l.tab = which)
            >
                {t(text)}
            </button>
        }
    };
    view! {
        <Show when=full>
            <div
                role="tablist"
                class="flex shrink-0 gap-1 border-b border-gray-200 bg-white px-2 dark:border-gray-800 dark:bg-gray-950"
            >
                {tab(CenterTab::Preview, "shell-tab-preview")}
                {tab(CenterTab::Code, "shell-tab-code")}
                {tab(CenterTab::Log, "shell-tab-log")}
            </div>
        </Show>
        {move || match current.get() {
            CenterTab::Code => view! { <CodePane /> }.into_any(),
            CenterTab::Log => view! { <LogPane /> }.into_any(),
            CenterTab::Preview => view! { <PreviewPane layout /> }.into_any(),
        }}
    }
}
