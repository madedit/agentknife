//! The panes' contents. Placeholders until their task fills them: files,
//! timeline and code (P1-SHELL-005).

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
