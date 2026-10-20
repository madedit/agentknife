//! The panes' contents. Placeholders until their tasks fill them: chat
//! (P1-SHELL-003), preview (P1-SHELL-004), files, timeline and code
//! (P1-SHELL-005).

use leptos::prelude::*;

use crate::i18n::t;

/// A pane section's heading.
#[component]
fn Heading(text: &'static str) -> impl IntoView {
    view! {
        <h2 class="px-4 pt-3 pb-2 text-xs font-semibold tracking-wide text-gray-500 uppercase dark:text-gray-400">
            {t(text)}
        </h2>
    }
}

/// Muted text for an empty pane.
#[component]
fn Empty(text: &'static str) -> impl IntoView {
    view! { <p class="px-4 text-sm text-gray-500 dark:text-gray-400">{t(text)}</p> }
}

#[component]
pub fn FilesPane() -> impl IntoView {
    view! {
        <section data-pane="file-list" class="min-h-0 flex-1 overflow-auto">
            <Heading text="shell-files" />
            <Empty text="shell-files-empty" />
        </section>
    }
}

#[component]
pub fn TimelinePane() -> impl IntoView {
    view! {
        <section
            data-pane="timeline"
            class="min-h-0 flex-1 overflow-auto border-t border-gray-200 dark:border-gray-800"
        >
            <Heading text="shell-timeline" />
            <Empty text="shell-timeline-empty" />
        </section>
    }
}

#[component]
pub fn PreviewPane() -> impl IntoView {
    view! {
        <div data-pane="preview" class="flex min-h-0 flex-1 items-center justify-center p-6">
            <div class="max-w-sm text-center">
                <p class="text-base font-medium">{t("shell-preview-empty")}</p>
                <p class="mt-2 text-sm text-gray-500 dark:text-gray-400">
                    {t("shell-preview-empty-hint")}
                </p>
            </div>
        </div>
    }
}

#[component]
pub fn CodePane() -> impl IntoView {
    view! {
        <div
            data-pane="code"
            class="flex min-h-0 flex-1 items-center justify-center bg-white p-6 dark:bg-gray-950"
        >
            <Empty text="shell-code-empty" />
        </div>
    }
}

#[component]
pub fn ChatPane() -> impl IntoView {
    view! {
        <Heading text="shell-chat" />
        <div class="min-h-0 flex-1 overflow-auto">
            <Empty text="shell-chat-empty" />
        </div>
        <div class="border-t border-gray-200 p-3 dark:border-gray-800">
            <textarea
                rows="3"
                disabled=true
                placeholder=t("shell-chat-placeholder")
                aria-label=t("shell-chat-placeholder")
                class="w-full resize-none rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm disabled:opacity-60 dark:border-gray-700 dark:bg-gray-900"
            ></textarea>
        </div>
    }
}
