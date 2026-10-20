//! The main window: a top bar and three panes (files + timeline | preview and
//! code | chat). Simple mode hides the files pane and the code tab.

use leptos::prelude::*;

use crate::i18n::t;
use crate::layout::{self, CHAT, CenterTab, LEFT, Layout, Mode};
use crate::panes::{ChatPane, CodePane, FilesPane, PreviewPane, TimelinePane};
use crate::splitter::{Grows, Splitter};

#[component]
pub fn Shell(
    /// Start from this layout instead of the saved one (tests).
    #[prop(optional)]
    initial: Option<Layout>,
) -> impl IntoView {
    let layout = RwSignal::new(initial.unwrap_or_else(layout::load));
    let full = move || layout.with(|l| l.mode == Mode::Full);

    view! {
        <div class="flex h-screen flex-col bg-white text-gray-900 dark:bg-gray-950 dark:text-gray-100">
            <TopBar layout />
            <div class="flex min-h-0 flex-1">
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
        </div>
    }
}

/// Changes the layout and saves it.
fn change(layout: RwSignal<Layout>, f: impl FnOnce(&mut Layout)) {
    layout.update(f);
    layout.with_untracked(layout::save);
}

#[component]
fn TopBar(layout: RwSignal<Layout>) -> impl IntoView {
    let mode_button = move |mode: Mode, text: &'static str, hint: &'static str| {
        let on = move || layout.with(|l| l.mode == mode);
        view! {
            <button
                type="button"
                data-mode=match mode {
                    Mode::Simple => "simple",
                    Mode::Full => "full",
                }
                aria-pressed=move || on().to_string()
                title=t(hint)
                class=move || {
                    if on() {
                        "rounded-md bg-white px-3 py-1 text-sm font-medium text-gray-900 shadow-sm dark:bg-gray-700 dark:text-white"
                    } else {
                        "rounded-md px-3 py-1 text-sm text-gray-600 hover:text-gray-900 dark:text-gray-400 dark:hover:text-white"
                    }
                }
                on:click=move |_| change(layout, |l| l.mode = mode)
            >
                {t(text)}
            </button>
        }
    };
    view! {
        <header class="flex h-11 shrink-0 items-center justify-between gap-4 border-b border-gray-200 px-4 dark:border-gray-800">
            <div class="flex min-w-0 items-center gap-3">
                <span class="font-semibold">{t("app-name")}</span>
                <span class="truncate text-sm text-gray-500 dark:text-gray-400">
                    {t("shell-no-project")}
                </span>
            </div>
            <div
                role="group"
                aria-label=t("shell-mode")
                class="flex shrink-0 gap-1 rounded-lg bg-gray-100 p-0.5 dark:bg-gray-800"
            >
                {mode_button(Mode::Simple, "shell-mode-simple", "shell-mode-simple-hint")}
                {mode_button(Mode::Full, "shell-mode-full", "shell-mode-full-hint")}
            </div>
        </header>
    }
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
