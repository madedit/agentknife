//! The tools check and the packaging result, as cards in the chat.

use ak_types::doctor::{CheckScope, CheckStatus, GuideStep, InstallPhase, SystemAction};
use ak_types::ipc::{BundleFailure, BundleResult};
use leptos::prelude::*;

use super::log::{BundleCard, DoctorCard};
use super::{CARD, Chat, PRIMARY, SECONDARY, send_message};
use crate::backend::{Backend, CallError};
use crate::i18n::{t, t_with, tr, tr_with};

fn message(e: CallError) -> String {
    match e {
        CallError::Connect(c) => c.to_string(),
        CallError::Other(d) => d,
    }
}

/// Runs the tools check into a new card (`fresh`) or the latest one.
pub fn check_tools(chat: Chat, backend: Backend, fresh: bool) {
    chat.log.update(|l| {
        let card = if fresh { l.new_doctor() } else { l.doctor() };
        card.busy = true;
        card.error = None;
    });
    leptos::task::spawn_local(async move {
        let result = backend.doctor_check().await;
        chat.log.update(|l| {
            let card = l.doctor();
            card.busy = false;
            match result {
                Ok(c) => card.check = Some(c),
                Err(e) => card.error = Some(message(e)),
            }
        });
    });
}

/// Checks quietly; adds a card only when something is missing (on project open).
pub fn check_tools_quietly(chat: Chat, backend: Backend) {
    leptos::task::spawn_local(async move {
        if let Ok(c) = backend.doctor_check().await
            && !c.report.ready()
        {
            chat.log.update(|l| {
                let card = l.new_doctor();
                card.busy = false;
                card.check = Some(c);
            });
        }
    });
}

fn install_tools(chat: Chat, backend: Backend) {
    chat.log.update(|l| {
        let card = l.doctor();
        card.busy = true;
        card.error = None;
    });
    leptos::task::spawn_local(async move {
        let result = backend.doctor_install().await;
        chat.log.update(|l| {
            let card = l.doctor();
            card.busy = false;
            card.progress = None;
            match result {
                Ok(c) => card.check = Some(c),
                Err(e) => card.error = Some(message(e)),
            }
        });
    });
}

/// Packages the app into a new card.
pub fn bundle(chat: Chat, backend: Backend) {
    chat.log.update(|l| l.bundle_started());
    leptos::task::spawn_local(async move {
        let result = backend.bundle().await;
        chat.log.update(|l| {
            l.bundle_finished(result.map_err(|e| {
                e.unwrap_or_else(|call| BundleFailure {
                    kind: "other".into(),
                    detail: message(call),
                })
            }));
        });
    });
}

fn status_key(s: CheckStatus) -> &'static str {
    match s {
        CheckStatus::Ok => "ok",
        CheckStatus::Missing => "missing",
        CheckStatus::WrongVersion => "wrong-version",
        CheckStatus::Error => "error",
    }
}

/// Copies `text` to the clipboard (browser only).
fn copy(text: &str) {
    #[cfg(target_arch = "wasm32")]
    if let Some(w) = web_sys::window() {
        let _ = w.navigator().clipboard().write_text(text);
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = text;
}

/// A guide step: its words, a command to copy, a page to open.
#[component]
fn Step(step: GuideStep, #[prop(optional)] args: Vec<(&'static str, String)>) -> impl IntoView {
    let copied = RwSignal::new(false);
    let text = tr_with(step.message.clone(), args);
    view! {
        <li>
            <p>{text}</p>
            {step.command.map(|c| {
                let c2 = c.clone();
                view! {
                    <div class="mt-1 flex items-center gap-2">
                        <code data-command="" class="min-w-0 flex-1 overflow-auto rounded bg-gray-100 px-2 py-1 text-xs whitespace-pre dark:bg-gray-800">{c}</code>
                        <button
                            type="button"
                            data-action="copy"
                            class="shrink-0 text-xs text-blue-700 hover:underline dark:text-blue-300"
                            on:click=move |_| {
                                copy(&c2);
                                copied.set(true);
                            }
                        >
                            {move || if copied.get() { t("doctor-copied")() } else { t("doctor-copy")() }}
                        </button>
                    </div>
                }
            })}
            {step.link.map(|l| {
                let href = l.clone();
                view! {
                    <a href=href target="_blank" rel="noreferrer" class="mt-1 block truncate text-xs text-blue-700 underline dark:text-blue-300">{l}</a>
                }
            })}
        </li>
    }
}

#[component]
pub fn DoctorCardView(card: DoctorCard) -> impl IntoView {
    let chat = expect_context::<Chat>();
    let backend = StoredValue::new(expect_context::<Backend>());
    let confirming = RwSignal::new(None::<SystemAction>);
    let busy = card.busy;
    let items = card.check.as_ref().map(|c| {
        c.report
            .items
            .iter()
            .map(|i| {
                let ok = i.status == CheckStatus::Ok;
                view! {
                    <li data-tool=i.id.clone() data-ok=ok.to_string() class="flex items-baseline gap-2">
                        <span aria-hidden="true" class=if ok { "w-4 text-green-600" } else { "w-4 text-amber-600" }>
                            {if ok { "✓" } else { "!" }}
                        </span>
                        <span class="flex-1">{tr(format!("doctor-item-{}", i.id))}</span>
                        <span class="text-xs text-gray-500 dark:text-gray-400">{tr(format!("doctor-status-{}", status_key(i.status)))}</span>
                    </li>
                }
            })
            .collect_view()
    });
    let ready = card.check.as_ref().is_some_and(|c| c.report.ready());
    let missing_isolated = card.check.as_ref().is_some_and(|c| {
        c.report
            .items
            .iter()
            .any(|i| i.scope == CheckScope::Isolated && i.status != CheckStatus::Ok)
    });
    let guides = card
        .check
        .as_ref()
        .map(|c| c.guides.clone())
        .unwrap_or_default();
    let progress = card.progress.clone().map(|p| {
        let tool = tr(format!("doctor-item-{}", p.item))();
        let label = tr_with(
            format!("doctor-install-{}", p.phase.key()),
            vec![
                ("tool", tool),
                ("step", p.step.to_string()),
                ("total", p.total.to_string()),
            ],
        );
        let percent = match p.phase {
            InstallPhase::Download {
                done,
                size: Some(size),
            } if size > 0 => Some(done.saturating_mul(100) / size),
            _ => None,
        };
        view! {
            <div data-progress="" role="status" class="mt-3">
                <p class="text-gray-700 dark:text-gray-300">{label}</p>
                {percent.map(|p| view! {
                    <div class="mt-1 h-1.5 overflow-hidden rounded bg-gray-200 dark:bg-gray-700">
                        <div class="h-full bg-blue-600" style=format!("width:{p}%")></div>
                    </div>
                })}
            </div>
        }
    });
    let guide_views = guides
        .into_iter()
        .map(|g| {
            let action = g.action.clone();
            let steps = g.steps.into_iter().map(|s| view! { <Step step=s /> }).collect_view();
            view! {
                <div data-guide=g.item.clone() class="mt-3 rounded-lg border border-amber-200 p-3 dark:border-amber-900">
                    <p class="font-medium">{tr(format!("doctor-item-{}", g.item))}</p>
                    <ol class="mt-2 list-decimal space-y-2 pl-5 text-gray-700 dark:text-gray-300">{steps}</ol>
                    {action.map(|a| {
                        let key = a.key();
                        let a2 = a.clone();
                        let a3 = a.clone();
                        let is_confirming = move || confirming.get().as_ref() == Some(&a2);
                        view! {
                            <Show
                                when=is_confirming
                                fallback=move || {
                                    let a = a3.clone();
                                    view! {
                                        <button type="button" data-action="system-action" class=format!("mt-3 {PRIMARY}") on:click=move |_| confirming.set(Some(a.clone()))>
                                            {tr(format!("doctor-action-{key}"))}
                                        </button>
                                    }
                                }
                            >
                                {
                                    let a = a.clone();
                                    view! {
                                        <div data-confirm="system-action" class="mt-3 rounded bg-amber-50 p-2 text-amber-900 dark:bg-amber-950 dark:text-amber-100">
                                            <p>{tr(format!("doctor-confirm-{key}"))}</p>
                                            <div class="mt-2 flex gap-2">
                                                <button
                                                    type="button"
                                                    data-action="confirm-system-action"
                                                    class=PRIMARY
                                                    on:click={
                                                        let a = a.clone();
                                                        move |_| {
                                                            confirming.set(None);
                                                            let a = a.clone();
                                                            let b = backend.get_value();
                                                            leptos::task::spawn_local(async move {
                                                                if let Err(e) = b.doctor_action(&a).await {
                                                                    chat.log.update(|l| l.doctor().error = Some(message(e)));
                                                                }
                                                            });
                                                        }
                                                    }
                                                >
                                                    {t("doctor-confirm-yes")}
                                                </button>
                                                <button type="button" class=SECONDARY on:click=move |_| confirming.set(None)>
                                                    {t("common-cancel")}
                                                </button>
                                            </div>
                                        </div>
                                    }
                                }
                            </Show>
                        }
                    })}
                </div>
            }
        })
        .collect_view();
    let has_guides = card.check.as_ref().is_some_and(|c| !c.guides.is_empty());
    view! {
        <div data-card="doctor" class=CARD>
            <p class="font-medium">{t("doctor-title")}</p>
            {(busy && card.check.is_none()).then(|| view! {
                <p class="mt-2 animate-pulse text-gray-500 dark:text-gray-400">{t("doctor-checking")}</p>
            })}
            {items.map(|i| view! { <ul class="mt-2 space-y-1">{i}</ul> })}
            {progress}
            {ready.then(|| view! { <p data-ready="" class="mt-3 text-green-700 dark:text-green-400">{t("doctor-ready")}</p> })}
            {(missing_isolated && !busy).then(|| view! {
                <p class="mt-3 text-gray-600 dark:text-gray-400">{t("doctor-isolated-note")}</p>
                <button type="button" data-action="install-tools" class=format!("mt-2 {PRIMARY}") on:click=move |_| install_tools(chat, backend.get_value())>
                    {t("doctor-install-button")}
                </button>
            })}
            {has_guides.then(|| view! { <p class="mt-3 text-gray-600 dark:text-gray-400">{t("doctor-system-intro")}</p> })}
            {guide_views}
            {(has_guides && !busy).then(|| view! {
                <button type="button" data-action="check-again" class=format!("mt-3 {SECONDARY}") on:click=move |_| check_tools(chat, backend.get_value(), false)>
                    {t("doctor-check-again")}
                </button>
            })}
            {card.error.map(|e| view! {
                <div role="alert" class="mt-3 text-red-800 dark:text-red-200">
                    <p>{t("doctor-error")}</p>
                    <details class="text-xs"><summary class="cursor-pointer">{t("common-details")}</summary><pre class="whitespace-pre-wrap">{e}</pre></details>
                </div>
            })}
        </div>
    }
}

const SYSTEMS: [&str; 3] = ["windows", "macos", "linux"];

#[component]
pub fn BundleCardView(card: BundleCard) -> impl IntoView {
    let chat = expect_context::<Chat>();
    let body = match card {
        BundleCard::Running { stage, lines } => view! {
            <p role="status" class="mt-2 animate-pulse text-gray-700 dark:text-gray-300">
                {stage.map(tr).map(|f| f()).unwrap_or_else(|| t("bundle-building")())}
            </p>
            {(!lines.is_empty()).then(|| view! {
                <details class="mt-2 text-xs text-gray-500 dark:text-gray-400">
                    <summary class="cursor-pointer select-none">{t("common-details")}</summary>
                    <pre class="mt-1 max-h-48 overflow-auto rounded bg-gray-100 p-2 whitespace-pre-wrap dark:bg-gray-800">{lines.join("\n")}</pre>
                </details>
            })}
        }
        .into_any(),
        BundleCard::Done(result) => view! { <BundleDone result /> }.into_any(),
        BundleCard::Failed(f) => {
            let detail = f.detail.clone();
            let ask = move |_| send_message(chat, t("bundle-fix-message")());
            view! {
                <p class="mt-2 text-red-800 dark:text-red-200">{tr(format!("bundle-error-{}", f.kind))}</p>
                {(f.kind == "build-failed" || f.kind == "preview-code").then(|| view! {
                    <button type="button" data-action="fix-bundle" class=format!("mt-3 {PRIMARY}") on:click=ask>
                        {t("preview-fix")}
                    </button>
                })}
                {(!detail.is_empty()).then(|| view! {
                    <details class="mt-2 text-xs text-gray-500 dark:text-gray-400">
                        <summary class="cursor-pointer select-none">{t("common-details")}</summary>
                        <pre class="mt-1 max-h-48 overflow-auto whitespace-pre-wrap">{detail}</pre>
                    </details>
                })}
            }
            .into_any()
        }
    };
    view! {
        <div data-card="bundle" class=CARD>
            <p class="font-medium">{t("bundle-title")}</p>
            {body}
        </div>
    }
}

#[component]
fn BundleDone(result: BundleResult) -> impl IntoView {
    let backend = StoredValue::new(expect_context::<Backend>());
    let other = RwSignal::new(None::<&'static str>);
    let args = vec![("file", result.file.clone()), ("app", result.app.clone())];
    let steps = result
        .steps
        .into_iter()
        .map(|s| view! { <Step step=s args=args.clone() /> })
        .collect_view();
    let path = result.path.clone();
    let host = result.os.clone();
    let others = SYSTEMS
        .into_iter()
        .filter(|os| *os != host)
        .map(|os| {
            view! {
                <button type="button" data-os=os class="text-xs text-blue-700 hover:underline dark:text-blue-300" on:click=move |_| other.set(Some(os))>
                    {tr(format!("os-{os}"))}
                </button>
            }
        })
        .collect_view();
    let host_name = tr(format!("os-{host}"));
    view! {
        <p data-done="" class="mt-2 text-green-700 dark:text-green-400">{t_with("bundle-done", [("file", result.file.clone())])}</p>
        <p class="mt-3 font-medium">{t("bundle-steps-title")}</p>
        <ol class="mt-2 list-decimal space-y-2 pl-5 text-gray-700 dark:text-gray-300">{steps}</ol>
        <button
            type="button"
            data-action="reveal"
            class=format!("mt-3 {SECONDARY}")
            on:click=move |_| {
                let (b, p) = (backend.get_value(), path.clone());
                leptos::task::spawn_local(async move {
                    let _ = b.reveal(&p).await;
                });
            }
        >
            {t("bundle-open-folder")}
        </button>
        <div class="mt-3 flex flex-wrap items-baseline gap-2 text-xs text-gray-500 dark:text-gray-400">
            <span>{t("bundle-other-os-question")}</span>
            {others}
        </div>
        {move || other.get().map(|os| {
            let requested = tr(format!("os-{os}"))();
            let host = host_name.clone()();
            view! {
                <p data-note="other-os" class="mt-2 rounded bg-gray-50 p-2 text-xs dark:bg-gray-800">
                    {t_with("bundle-other-os", [("host", host), ("requested", requested)])}
                </p>
            }
        })}
    }
}
