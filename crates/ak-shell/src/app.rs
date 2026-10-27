//! Which screen shows: the connect wizard on first run, the home screen, or a
//! project's workspace; plus the top bar they share.

use ak_types::ipc::{HomeState, OpenedProject, ProjectRow};
use ak_types::settings::AiConnection;
use leptos::prelude::*;

use crate::backend::{Backend, CallError};
use crate::chat::Chat;
use crate::chat::log::ChatLog;
use crate::connect::ConnectWizard;
use crate::files::Files;
use crate::home::HomeScreen;
use crate::i18n::{self, t, t_with, tr};
use crate::layout::{self, Layout, Mode};
use crate::logs::ProjectLog;
use crate::preview::Preview;
use crate::shell::{Workspace, change};

#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    Loading,
    /// The app could not be reached.
    Failed(String),
    /// `back`: there is a screen to return to (changing the AI later).
    Connect {
        back: bool,
    },
    Home,
    Workspace(ProjectRow),
    Settings,
}

/// The whole UI, in the browser's language, talking to the app (or the demo).
#[component]
pub fn App() -> impl IntoView {
    i18n::provide(i18n::browser_locale());
    let backend = Backend::detect();
    provide_context(backend.clone());
    let chat = Chat::new(ChatLog::default());
    provide_context(chat);
    let files = Files::new();
    provide_context(files);
    let b = backend.clone();
    backend.listen(move |event| {
        // A finished turn may have changed files and added a version.
        let finished = matches!(event, ak_types::agent::AgentEvent::TurnFinished { .. });
        chat.log.update(|l| l.apply(event));
        if finished {
            files.refresh(b.clone());
        }
    });
    let preview = Preview::new();
    provide_context(preview);
    backend.listen_preview(move |status| preview.status.set(status));
    backend.on(ak_types::ipc::PREVIEW_RELOAD_EVENT, move |()| {
        preview.reloads.update(|n| *n += 1);
    });
    let project_log = ProjectLog::new();
    provide_context(project_log);
    backend.listen_log(move |batch| project_log.append(batch));
    backend.on(ak_types::ipc::DOCTOR_EVENT, move |p| {
        chat.log.update(|l| l.doctor_progress(p));
    });
    backend.on(ak_types::ipc::BUNDLE_EVENT, move |p| {
        chat.log.update(|l| l.bundle_progress(p));
    });
    // Checks and packaging another AI tool ran.
    backend.on(ak_types::ipc::DOCTOR_CHECKED_EVENT, move |c| {
        chat.log.update(|l| l.doctor_checked(c));
    });
    backend.on(ak_types::ipc::BUNDLE_STARTED_EVENT, move |()| {
        chat.log.update(|l| l.bundle_started());
    });
    backend.on(ak_types::ipc::BUNDLE_FINISHED_EVENT, move |r| {
        chat.log.update(|l| l.bundle_finished(r));
    });
    let external = crate::external::External::new();
    provide_context(external);
    external.follow(&backend);
    // Another AI tool changed files or made a version.
    {
        let b = backend.clone();
        backend.on(ak_types::ipc::PROJECT_CHANGED_EVENT, move |()| {
            files.refresh(b.clone());
        });
    }
    let home = RwSignal::new(HomeState::default());
    let screen = RwSignal::new(Screen::Loading);
    reload(&backend, home, screen, true);
    // The language chosen in the settings replaces the browser's.
    {
        let backend = backend.clone();
        let tr = i18n::Tr::get();
        leptos::task::spawn_local(async move {
            if let Ok(s) = backend.get_settings().await
                && let Some(l) = s.language
            {
                tr.set_locale(l);
            }
        });
    }
    view! { <AppView screen home /> }
}

/// Fetches the home state; `route` also picks the screen (connect when no AI).
fn reload(backend: &Backend, home: RwSignal<HomeState>, screen: RwSignal<Screen>, route: bool) {
    let backend = backend.clone();
    leptos::task::spawn_local(async move {
        match backend.home_state().await {
            Ok(h) => {
                // Another AI tool may do the work instead of a connected one.
                let connected = h.ai.is_some() || h.external;
                home.set(h);
                if route {
                    screen.set(if connected {
                        Screen::Home
                    } else {
                        Screen::Connect { back: false }
                    });
                }
            }
            Err(CallError::Connect(e)) => screen.set(Screen::Failed(e.to_string())),
            Err(CallError::Other(e)) => screen.set(Screen::Failed(e)),
        }
    });
}

/// The UI for a given state (tests render it directly).
#[component]
pub fn AppView(
    screen: RwSignal<Screen>,
    home: RwSignal<HomeState>,
    #[prop(optional)] layout: Option<Layout>,
) -> impl IntoView {
    let layout = RwSignal::new(layout.unwrap_or_else(layout::load));
    let backend = expect_context::<Backend>();
    let chat = use_context::<Chat>().unwrap_or_else(|| {
        let c = Chat::new(ChatLog::default());
        provide_context(c);
        c
    });
    let files = use_context::<Files>().unwrap_or_else(|| {
        let f = Files::new();
        provide_context(f);
        f
    });
    let preview = use_context::<Preview>().unwrap_or_else(|| {
        let p = Preview::new();
        provide_context(p);
        p
    });
    if use_context::<ProjectLog>().is_none() {
        provide_context(ProjectLog::new());
    }
    // Where "Cancel" in the wizard goes back to.
    let before_connect = StoredValue::new(Screen::Home);

    let go_home = {
        let backend = backend.clone();
        Callback::new(move |()| {
            reload(&backend, home, screen, false);
            screen.set(Screen::Home);
        })
    };
    let connected = {
        let backend = backend.clone();
        Callback::new(move |_: AiConnection| {
            reload(&backend, home, screen, false);
            screen.set(before_connect.get_value());
        })
    };
    // Another AI tool connected in the wizard.
    let external_done = {
        let backend = backend.clone();
        Callback::new(move |()| {
            reload(&backend, home, screen, false);
            screen.set(before_connect.get_value());
        })
    };
    let change_ai = Callback::new(move |()| {
        let now = screen.get_untracked();
        if !matches!(now, Screen::Connect { .. }) {
            before_connect.set_value(now);
        }
        screen.set(Screen::Connect { back: true });
    });
    // Where "Back" on the settings page goes.
    let before_settings = StoredValue::new(Screen::Home);
    let open_settings = Callback::new(move |()| {
        let now = screen.get_untracked();
        if !matches!(now, Screen::Settings | Screen::Connect { .. }) {
            before_settings.set_value(now);
        }
        screen.set(Screen::Settings);
    });
    let open = {
        let backend = backend.clone();
        Callback::new(move |o: OpenedProject| {
            chat.log
                .set(ChatLog::from_history(&o.history, o.interrupted));
            chat.selection.set(None);
            preview.picking.set(false);
            files.reset();
            files.refresh(backend.clone());
            crate::chat::check_tools_quietly(chat, backend.clone());
            let backend = backend.clone();
            leptos::task::spawn_local(async move {
                if let Ok(status) = backend.preview_status().await {
                    preview.status.set(status);
                }
            });
            screen.set(Screen::Workspace(o.project));
        })
    };
    // Another AI tool opened a project.
    backend.on(
        ak_types::ipc::PROJECT_OPENED_EVENT,
        move |o: OpenedProject| {
            open.run(o);
        },
    );

    view! {
        <div class="flex h-screen flex-col bg-white text-gray-900 dark:bg-gray-950 dark:text-gray-100">
            <TopBar screen home layout go_home change_ai open_settings demo=backend.is_demo() />
            {move || match screen.get() {
                Screen::Loading => view! {
                    <p class="m-auto text-sm text-gray-500 dark:text-gray-400">{t("shell-loading")}</p>
                }.into_any(),
                Screen::Failed(detail) => view! {
                    <div role="alert" class="m-auto max-w-md text-center">
                        <p class="font-medium">{t("shell-failed")}</p>
                        <details class="mt-2 text-sm">
                            <summary class="cursor-pointer">{t("common-details")}</summary>
                            <pre class="mt-1 text-xs whitespace-pre-wrap">{detail}</pre>
                        </details>
                    </div>
                }.into_any(),
                Screen::Connect { back } => {
                    let cancel = back.then(|| Callback::new(move |()| {
                        screen.set(before_connect.get_value());
                    }));
                    match cancel {
                        Some(c) => view! { <ConnectWizard on_done=connected on_external=external_done on_cancel=c /> }.into_any(),
                        None => view! { <ConnectWizard on_done=connected on_external=external_done /> }.into_any(),
                    }
                }
                Screen::Home => view! {
                    <HomeScreen home now=crate::backend::now() on_open=open />
                }.into_any(),
                Screen::Workspace(_) => view! { <Workspace layout /> }.into_any(),
                Screen::Settings => {
                    let back = before_settings.get_value();
                    view! {
                        <crate::settings::SettingsScreen
                            ai=Signal::derive(move || home.with(|h| h.ai.clone()))
                            change_ai=change_ai
                            on_close=Callback::new(move |()| screen.set(back.clone()))
                        />
                    }.into_any()
                }
            }}
        </div>
    }
}

#[component]
fn TopBar(
    screen: RwSignal<Screen>,
    home: RwSignal<HomeState>,
    layout: RwSignal<Layout>,
    go_home: Callback<()>,
    change_ai: Callback<()>,
    open_settings: Callback<()>,
    demo: bool,
) -> impl IntoView {
    let project = move || match screen.get() {
        Screen::Workspace(row) => Some(row.name),
        _ => None,
    };
    let in_workspace = move || project().is_some();
    let external = crate::external::External::get();
    let ai_label = move || {
        let own = home.with(|h| h.ai.clone()).map(|ai| {
            let provider = tr(format!("connect-preset-{}", ai.provider))();
            t_with("shell-ai", [("provider", provider), ("model", ai.model)])()
        });
        let other = external
            .names()
            .map(|names| t_with("shell-ai-external", [("names", names)])());
        // The tool doing the work comes first (the label is cut off when long).
        match (other, own) {
            (Some(other), Some(own)) => format!("{other} · {own}"),
            (Some(one), None) | (None, Some(one)) => one,
            (None, None) => t("shell-ai-none")(),
        }
    };
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
                <Show when=in_workspace>
                    <button
                        title=t("shell-projects-tip")
                        type="button"
                        data-action="home"
                        class="text-sm text-blue-700 hover:underline dark:text-blue-300"
                        on:click=move |_| go_home.run(())
                    >
                        {t("shell-projects")}
                    </button>
                </Show>
                <span class="truncate text-sm text-gray-500 dark:text-gray-400">
                    {move || project().unwrap_or_else(|| t("shell-no-project")())}
                </span>
                {demo.then(|| view! {
                    <span data-badge="demo" title=t("shell-demo-hint") class="shrink-0 rounded bg-amber-100 px-2 py-0.5 text-xs text-amber-900 dark:bg-amber-900 dark:text-amber-100">
                        {t("shell-demo")}
                    </span>
                })}
            </div>
            <div class="flex shrink-0 items-center gap-3">
                <Show when=move || !matches!(screen.get(), Screen::Connect { .. } | Screen::Settings)>
                    <button
                        title=t("settings-tip")
                        type="button"
                        data-action="settings"
                        class="text-sm text-gray-600 hover:text-gray-900 dark:text-gray-400 dark:hover:text-white"
                        on:click=move |_| open_settings.run(())
                    >
                        {t("settings-title")}
                    </button>
                </Show>
                // Not on the wizard itself (on first run there is nothing to go back to).
                <Show when=move || !matches!(screen.get(), Screen::Connect { .. })>
                <button
                    type="button"
                    data-action="change-ai"
                    title=t("shell-ai-change")
                    class="max-w-64 truncate rounded-full border border-gray-300 px-3 py-0.5 text-xs text-gray-700 hover:bg-gray-100 dark:border-gray-700 dark:text-gray-300 dark:hover:bg-gray-800"
                    on:click=move |_| change_ai.run(())
                >
                    {ai_label}
                </button>
                </Show>
                <Show when=in_workspace>
                    <div
                        role="group"
                        aria-label=t("shell-mode")
                        class="flex gap-1 rounded-lg bg-gray-100 p-0.5 dark:bg-gray-800"
                    >
                        {mode_button(Mode::Simple, "shell-mode-simple", "shell-mode-simple-hint")}
                        {mode_button(Mode::Full, "shell-mode-full", "shell-mode-full-hint")}
                    </div>
                </Show>
            </div>
        </header>
    }
}
