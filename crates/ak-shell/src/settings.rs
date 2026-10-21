//! The settings page: language, AI, permissions, folders the AI may read,
//! plans, automatic fixes, version history, daily limit, privacy and crash
//! reports.

use ak_types::i18n::Locale;
use ak_types::ipc::CrashReports;
use ak_types::settings::{AiConnection, AppSettings, Permissions, Plans};
use leptos::prelude::*;

use crate::backend::Backend;
use crate::chat::tools::copy;
use crate::files::ago;
use crate::i18n::{self, t, t_count, t_with, tr};

const SECTION: &str =
    "rounded-xl border border-gray-200 bg-white p-4 dark:border-gray-700 dark:bg-gray-950";
const FIELD: &str = "rounded-lg border border-gray-300 bg-white px-2 py-1 text-sm dark:border-gray-700 dark:bg-gray-900";

/// A section with a title and an explanation.
#[component]
fn Section(
    title: &'static str,
    #[prop(optional)] about: Option<&'static str>,
    children: Children,
) -> impl IntoView {
    view! {
        <section class=SECTION>
            <h2 class="font-medium">{t(title)}</h2>
            {about.map(|a| view! { <p class="mt-1 text-sm text-gray-600 dark:text-gray-400">{t(a)}</p> })}
            <div class="mt-3">{children()}</div>
        </section>
    }
}

/// Radio buttons for one setting.
#[component]
fn Choice<T: Copy + PartialEq + Send + Sync + 'static>(
    name: &'static str,
    options: Vec<(T, &'static str, Option<&'static str>)>,
    value: Signal<T>,
    set: Callback<T>,
) -> impl IntoView {
    options
        .into_iter()
        .map(|(v, label, about)| {
            view! {
                <label class="flex items-start gap-2 py-1 text-sm">
                    <input
                        type="radio"
                        name=name
                        data-choice=label
                        checked=move || value.get() == v
                        prop:checked=move || value.get() == v
                        on:change=move |_| set.run(v)
                    />
                    <span>
                        <span class="block">{t(label)}</span>
                        {about.map(|a| view! { <span class="block text-xs text-gray-500 dark:text-gray-400">{t(a)}</span> })}
                    </span>
                </label>
            }
        })
        .collect_view()
}

#[component]
pub fn SettingsScreen(
    /// The AI in use (from the home state).
    ai: Signal<Option<AiConnection>>,
    change_ai: Callback<()>,
    on_close: Callback<()>,
    /// Start from these instead of asking the app (tests).
    #[prop(optional)]
    initial: Option<AppSettings>,
    #[prop(optional)] crash: Option<CrashReports>,
) -> impl IntoView {
    let backend = StoredValue::new(expect_context::<Backend>());
    let loaded = initial.is_some();
    let s = RwSignal::new(initial.unwrap_or_default());
    let reports = RwSignal::new(crash);
    let saved = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    // The limit as typed (an empty box means no limit).
    let cap_text = RwSignal::new(
        s.with_untracked(|s| s.daily_cap_usd.map(|c| c.to_string()).unwrap_or_default()),
    );
    if !loaded {
        leptos::task::spawn_local(async move {
            let b = backend.get_value();
            if let Ok(v) = b.get_settings().await {
                cap_text.set(v.daily_cap_usd.map(|c| c.to_string()).unwrap_or_default());
                s.set(v);
            }
            if let Ok(r) = b.crash_reports().await {
                reports.set(Some(r));
            }
        });
    }
    let change = move |f: Box<dyn FnOnce(&mut AppSettings)>| {
        s.update(|s| f(s));
        saved.set(false);
    };
    let save = move |_| {
        let cap = cap_text.get_untracked().trim().replace(',', ".");
        let cap = if cap.is_empty() {
            None
        } else {
            cap.parse::<f64>().ok()
        };
        let value = AppSettings {
            daily_cap_usd: cap,
            ..s.get_untracked()
        }
        .clamped();
        s.set(value.clone());
        cap_text.set(
            value
                .daily_cap_usd
                .map(|c| c.to_string())
                .unwrap_or_default(),
        );
        let b = backend.get_value();
        let tr = i18n::Tr::get();
        leptos::task::spawn_local(async move {
            match b.save_settings(&value).await {
                Ok(()) => {
                    tr.set_locale(i18n::chosen_locale(value.language));
                    error.set(None);
                    saved.set(true);
                }
                Err(e) => error.set(Some(format!("{e:?}"))),
            }
        });
    };
    let language = move || s.with(|s| s.language.map(|l| l.tag().to_owned()).unwrap_or_default());
    let now = crate::backend::now();

    view! {
        <div data-screen="settings" class="flex min-h-0 flex-1 justify-center overflow-auto bg-gray-50 p-6 dark:bg-gray-900">
            <div class="flex w-full max-w-2xl flex-col gap-4">
                <div class="flex items-center justify-between gap-3">
                    <h1 class="text-2xl font-semibold">{t("settings-title")}</h1>
                    <button type="button" data-action="close-settings" class="text-sm text-blue-700 hover:underline dark:text-blue-300" on:click=move |_| on_close.run(())>
                        {t("connect-back")}
                    </button>
                </div>

                <Section title="settings-language">
                    <select
                        data-setting="language"
                        class=FIELD
                        on:change=move |e| {
                            let v = event_target_value(&e);
                            change(Box::new(move |s| s.language = v.parse::<Locale>().ok()));
                        }
                    >
                        <option value="" selected=move || language().is_empty()>{t("settings-language-system")}</option>
                        {Locale::ALL.into_iter().map(|l| view! {
                            <option value=l.tag() selected=move || language() == l.tag()>{tr(format!("language-{}", l.tag()))}</option>
                        }).collect_view()}
                    </select>
                </Section>

                <Section title="settings-ai">
                    <div class="flex flex-wrap items-center justify-between gap-2 text-sm">
                        <span data-setting="ai">
                            {move || match ai.get() {
                                Some(a) => {
                                    let provider = tr(format!("connect-preset-{}", a.provider))();
                                    t_with("shell-ai", [("provider", provider), ("model", a.model)])()
                                }
                                None => t("shell-ai-none")(),
                            }}
                        </span>
                        <button type="button" data-action="settings-change-ai" class="rounded-lg border border-gray-300 px-3 py-1 hover:bg-gray-100 dark:border-gray-700 dark:hover:bg-gray-800" on:click=move |_| change_ai.run(())>
                            {t("shell-ai-change")}
                        </button>
                    </div>
                </Section>

                <Section title="settings-permissions">
                    <Choice
                        name="permissions"
                        options=vec![
                            (Permissions::Standard, "settings-permissions-standard", Some("settings-permissions-standard-about")),
                            (Permissions::AskEveryChange, "settings-permissions-ask", Some("settings-permissions-ask-about")),
                        ]
                        value=Signal::derive(move || s.with(|s| s.permissions))
                        set=Callback::new(move |v| change(Box::new(move |s| s.permissions = v)))
                    />
                </Section>

                <Section title="settings-read" about="settings-read-about">
                    {move || {
                        let folders = s.with(|s| s.read_folders.clone());
                        if folders.is_empty() {
                            return view! { <p data-read-none="" class="text-sm text-gray-500">{t("settings-read-none")}</p> }.into_any();
                        }
                        view! {
                            <ul class="text-sm">
                                {folders.into_iter().map(|f| {
                                    let (gone, attr, title) = (f.clone(), f.clone(), f.clone());
                                    view! {
                                        <li data-read-folder=attr class="flex items-center justify-between gap-2 py-1">
                                            <span class="min-w-0 truncate font-mono text-xs" title=title>{f}</span>
                                            <button
                                                type="button"
                                                data-action="remove-read-folder"
                                                class="shrink-0 text-blue-700 hover:underline dark:text-blue-300"
                                                on:click=move |_| {
                                                    let gone = gone.clone();
                                                    change(Box::new(move |s| s.read_folders.retain(|x| *x != gone)));
                                                }
                                            >
                                                {t("settings-read-remove")}
                                            </button>
                                        </li>
                                    }
                                }).collect_view()}
                            </ul>
                        }.into_any()
                    }}
                    <button
                        type="button"
                        data-action="add-read-folder"
                        class="mt-2 rounded-lg border border-gray-300 px-3 py-1 text-sm hover:bg-gray-100 dark:border-gray-700 dark:hover:bg-gray-800"
                        on:click=move |_| {
                            let b = backend.get_value();
                            leptos::task::spawn_local(async move {
                                if let Ok(Some(f)) = b.pick_folder().await {
                                    change(Box::new(move |s| {
                                        if !s.read_folders.contains(&f) {
                                            s.read_folders.push(f);
                                        }
                                    }));
                                }
                            });
                        }
                    >
                        {t("settings-read-add")}
                    </button>
                </Section>

                <Section title="settings-plans" about="settings-plans-about">
                    <Choice
                        name="plans"
                        options=vec![
                            (Plans::BigChanges, "settings-plans-big", None),
                            (Plans::Always, "settings-plans-always", None),
                            (Plans::Never, "settings-plans-never", None),
                        ]
                        value=Signal::derive(move || s.with(|s| s.plans))
                        set=Callback::new(move |v| change(Box::new(move |s| s.plans = v)))
                    />
                </Section>

                <Section title="settings-fix" about="settings-fix-about">
                    <label class="flex items-center gap-2 text-sm">
                        <input
                            type="number"
                            data-setting="fix"
                            min=*AppSettings::FIX_ATTEMPTS.start()
                            max=*AppSettings::FIX_ATTEMPTS.end()
                            class=format!("w-20 {FIELD}")
                            value=move || s.with(|s| s.max_fix_attempts.to_string())
                            prop:value=move || s.with(|s| s.max_fix_attempts.to_string())
                            on:input=move |e| {
                                if let Ok(n) = event_target_value(&e).parse::<u32>() {
                                    change(Box::new(move |s| s.max_fix_attempts = n));
                                }
                            }
                        />
                        {t("settings-fix-unit")}
                    </label>
                </Section>

                <Section title="settings-history" about="settings-history-about">
                    <label class="flex items-center gap-2 text-sm">
                        <input
                            type="number"
                            data-setting="history"
                            min=*AppSettings::KEEP_TURNS.start()
                            max=*AppSettings::KEEP_TURNS.end()
                            class=format!("w-24 {FIELD}")
                            value=move || s.with(|s| s.keep_turns.to_string())
                            prop:value=move || s.with(|s| s.keep_turns.to_string())
                            on:input=move |e| {
                                if let Ok(n) = event_target_value(&e).parse::<u32>() {
                                    change(Box::new(move |s| s.keep_turns = n));
                                }
                            }
                        />
                        {t("settings-history-unit")}
                    </label>
                </Section>

                <Section title="settings-cap" about="settings-cap-about">
                    <label class="flex items-center gap-2 text-sm">
                        {t("settings-cap-currency")}
                        <input
                            type="text"
                            inputmode="decimal"
                            data-setting="cap"
                            placeholder=t("settings-cap-none")
                            class=format!("w-28 {FIELD}")
                            value=move || cap_text.get()
                            prop:value=move || cap_text.get()
                            on:input=move |e| {
                                cap_text.set(event_target_value(&e));
                                saved.set(false);
                            }
                        />
                    </label>
                </Section>

                <Section title="settings-privacy">
                    <p class="text-sm text-gray-700 dark:text-gray-300">{t("settings-privacy-text")}</p>
                    <h3 class="mt-4 text-sm font-medium">{t("settings-crash")}</h3>
                    <p class="mt-1 text-sm text-gray-600 dark:text-gray-400">{t("settings-crash-about")}</p>
                    {move || reports.get().map(|r| {
                        let folder = r.folder.clone();
                        let list = r.reports.iter().take(5).map(|c| {
                            let (id, n) = ago(c.created_at, now);
                            let text = c.text.clone();
                            let copied = RwSignal::new(false);
                            view! {
                                <li data-crash="" class="flex items-center justify-between gap-2 py-1">
                                    <span class="text-gray-600 dark:text-gray-400">{t_count(id, "n", n)}</span>
                                    <button
                                        type="button"
                                        data-action="copy-crash"
                                        class="text-blue-700 hover:underline dark:text-blue-300"
                                        on:click=move |_| {
                                            copy(&text);
                                            copied.set(true);
                                        }
                                    >
                                        {move || if copied.get() { t("doctor-copied")() } else { t("settings-crash-copy")() }}
                                    </button>
                                </li>
                            }
                        }).collect_view();
                        let empty = r.reports.is_empty();
                        view! {
                            {if empty {
                                view! { <p data-crash-none="" class="mt-2 text-sm text-gray-500">{t("settings-crash-none")}</p> }.into_any()
                            } else {
                                view! { <ul class="mt-2 text-sm">{list}</ul> }.into_any()
                            }}
                            <button
                                type="button"
                                data-action="show-crash-folder"
                                class="mt-2 text-sm text-blue-700 hover:underline dark:text-blue-300"
                                on:click=move |_| {
                                    let (b, f) = (backend.get_value(), folder.clone());
                                    leptos::task::spawn_local(async move {
                                        let _ = b.reveal(&f).await;
                                    });
                                }
                            >
                                {t("bundle-open-folder")}
                            </button>
                        }
                    })}
                </Section>

                <div class="flex items-center justify-end gap-3 pb-6">
                    <Show when=move || saved.get()>
                        <span data-saved="" class="text-sm text-green-700 dark:text-green-400">{t("settings-saved")}</span>
                    </Show>
                    {move || error.get().map(|e| view! { <span role="alert" class="text-sm text-red-700">{t("home-error")}" "{e}</span> })}
                    <button type="button" data-action="save-settings" class="rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700" on:click=save>
                        {t("settings-save")}
                    </button>
                </div>
            </div>
        </div>
    }
}
