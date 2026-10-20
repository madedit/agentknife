//! The home screen: recent projects, a new project, or a folder to open.

use ak_types::ipc::{HomeState, OpenedProject, ProjectRow};
use leptos::prelude::*;

use crate::backend::{Backend, CallError};
use crate::i18n::{t, t_count, t_with};

const PRIMARY: &str = "rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 disabled:opacity-50";
const SECONDARY: &str = "rounded-lg border border-gray-300 bg-white px-4 py-2 text-sm hover:bg-gray-100 dark:border-gray-700 dark:bg-gray-950 dark:hover:bg-gray-800";

/// How long ago, in days (0 = today).
pub fn days_ago(then: u64, now: u64) -> u64 {
    now.saturating_sub(then) / 86_400
}

#[component]
pub fn HomeScreen(
    home: Signal<HomeState>,
    /// Unix seconds, for "opened 3 days ago".
    now: u64,
    on_open: Callback<OpenedProject>,
) -> impl IntoView {
    let creating = RwSignal::new(false);
    let name = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);
    let backend = expect_context::<Backend>();

    let report = move |e: CallError| {
        error.set(Some(match e {
            CallError::Connect(c) => c.to_string(),
            CallError::Other(d) => d,
        }))
    };
    let create = {
        let backend = backend.clone();
        move || {
            let n = name.get_untracked().trim().to_owned();
            if n.is_empty() {
                return;
            }
            let backend = backend.clone();
            leptos::task::spawn_local(async move {
                match backend.create_project(&n).await {
                    Ok(opened) => on_open.run(opened),
                    Err(e) => report(e),
                }
            });
        }
    };
    let open_folder = {
        let backend = backend.clone();
        move |_| {
            let backend = backend.clone();
            leptos::task::spawn_local(async move {
                match backend.open_folder().await {
                    Ok(Some(opened)) => on_open.run(opened),
                    Ok(None) => {}
                    Err(e) => report(e),
                }
            });
        }
    };
    let open = move |row: ProjectRow| {
        let backend = backend.clone();
        leptos::task::spawn_local(async move {
            match backend.open_project(&row.id).await {
                Ok(opened) => on_open.run(opened),
                Err(e) => report(e),
            }
        });
    };

    let rows = move || {
        home.with(|h| h.projects.clone())
            .into_iter()
            .map(|row| {
                let days = days_ago(row.last_opened_at, now);
                let when = match days {
                    0 => t("home-opened-today")(),
                    1 => t("home-opened-yesterday")(),
                    n => t_count("home-opened-days", "days", n)(),
                };
                let exists = row.exists;
                let open = open.clone();
                let r = row.clone();
                view! {
                    <li>
                        <button
                            type="button"
                            data-project=row.id.clone()
                            disabled=!exists
                            class="flex w-full items-center justify-between gap-4 px-4 py-3 text-left hover:bg-gray-50 disabled:cursor-not-allowed disabled:opacity-60 dark:hover:bg-gray-900"
                            on:click=move |_| open(r.clone())
                        >
                            <span class="min-w-0">
                                <span class="block truncate font-medium">{row.name.clone()}</span>
                                <span class="block truncate text-xs text-gray-500 dark:text-gray-400">
                                    {row.path.clone()}
                                </span>
                            </span>
                            <span class="shrink-0 text-xs text-gray-500 dark:text-gray-400">
                                {if exists { when } else { t("home-missing")() }}
                            </span>
                        </button>
                    </li>
                }
            })
            .collect_view()
    };
    let create2 = create.clone();

    view! {
        <div data-screen="home" class="flex min-h-0 flex-1 justify-center overflow-auto bg-gray-50 p-6 dark:bg-gray-900">
            <div class="w-full max-w-2xl">
                <div class="flex flex-wrap items-center justify-between gap-3">
                    <h1 class="text-2xl font-semibold">{t("home-title")}</h1>
                    <div class="flex gap-2">
                        <button type="button" data-action="open-folder" class=SECONDARY on:click=open_folder>
                            {t("home-open-folder")}
                        </button>
                        <button
                            type="button"
                            data-action="new"
                            class=PRIMARY
                            on:click=move |_| {
                                name.set(String::new());
                                creating.set(true);
                            }
                        >
                            {t("home-new")}
                        </button>
                    </div>
                </div>
                <Show when=move || creating.get()>
                    <div data-dialog="new-project" class="mt-4 rounded-xl border border-gray-200 bg-white p-4 dark:border-gray-700 dark:bg-gray-950">
                        <label class="block text-sm font-medium">
                            {t("home-new-name")}
                            <input
                                type="text"
                                class="mt-1 w-full rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm dark:border-gray-700 dark:bg-gray-950"
                                prop:value=move || name.get()
                                on:input=move |e| name.set(event_target_value(&e))
                                on:keydown={
                                    let create = create.clone();
                                    move |e| if e.key() == "Enter" { create() }
                                }
                            />
                        </label>
                        <p class="mt-1 text-xs text-gray-500 dark:text-gray-400">
                            {move || t_with("home-new-where", [("folder", home.with(|h| h.projects_folder.clone()))])()}
                        </p>
                        <div class="mt-3 flex gap-2">
                            <button type="button" class=SECONDARY on:click=move |_| creating.set(false)>
                                {t("common-cancel")}
                            </button>
                            <button
                                type="button"
                                data-action="create"
                                class=PRIMARY
                                disabled=move || name.with(|n| n.trim().is_empty())
                                on:click={
                                    let create = create2.clone();
                                    move |_| create()
                                }
                            >
                                {t("home-create")}
                            </button>
                        </div>
                    </div>
                </Show>
                {move || error.get().map(|e| view! {
                    <div role="alert" class="mt-4 rounded-lg border border-red-300 bg-red-50 p-3 text-sm text-red-900 dark:border-red-800 dark:bg-red-950 dark:text-red-100">
                        <p>{t("home-error")}</p>
                        <details class="mt-1">
                            <summary class="cursor-pointer">{t("common-details")}</summary>
                            <pre class="mt-1 text-xs whitespace-pre-wrap">{e}</pre>
                        </details>
                    </div>
                })}
                <h2 class="mt-8 text-sm font-semibold text-gray-500 dark:text-gray-400">{t("home-recent")}</h2>
                <Show
                    when=move || home.with(|h| !h.projects.is_empty())
                    fallback=|| view! {
                        <p data-empty="projects" class="mt-3 text-sm text-gray-600 dark:text-gray-400">{t("home-empty")}</p>
                    }
                >
                    <ul class="mt-3 divide-y divide-gray-200 overflow-hidden rounded-xl border border-gray-200 bg-white dark:divide-gray-800 dark:border-gray-700 dark:bg-gray-950">
                        {rows.clone()}
                    </ul>
                </Show>
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_count_whole_days() {
        assert_eq!(days_ago(1_000, 1_000), 0);
        assert_eq!(days_ago(0, 86_399), 0);
        assert_eq!(days_ago(0, 86_400 * 3 + 5), 3);
        assert_eq!(days_ago(2_000, 1_000), 0, "clock moved back");
    }
}
