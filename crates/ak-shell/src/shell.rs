//! A project's workspace: three panes (files + timeline | preview and code |
//! chat). Simple mode hides the files pane and the code tab.

use leptos::prelude::*;

use crate::i18n::t;
use crate::layout::{self, CHAT, CenterTab, LEFT, Layout, Mode};
use crate::panes::{ChatPane, CodePane, FilesPane, PreviewPane, TimelinePane};
use crate::splitter::{Grows, Splitter};

#[component]
pub fn Workspace(layout: RwSignal<Layout>) -> impl IntoView {
    let full = move || layout.with(|l| l.mode == Mode::Full);

    view! {
        <div data-screen="workspace" class="flex min-h-0 flex-1">
                <Show when=full>
                    <aside
                        data-pane="files"
                        class="flex shrink-0 flex-col bg-gray-50 dark:bg-gray-900"
                        style:width=move || format!("min({}px, 30vw)", layout.with(|l| l.left))
                    >
                        <FilesPane />
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

/// The preview, plus the code tab in full mode.
#[component]
fn Center(layout: RwSignal<Layout>) -> impl IntoView {
    let full = move || layout.with(|l| l.mode == Mode::Full);
    let showing = move |tab: CenterTab| full() && layout.with(|l| l.tab == tab);
    let tab = move |which: CenterTab, text: &'static str| {
        let on = move || layout.with(|l| l.tab == which);
        view! {
            <button
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
            </div>
        </Show>
        <Show
            when=move || showing(CenterTab::Code)
            fallback=|| view! { <PreviewPane /> }
        >
            <CodePane />
        </Show>
    }
}
