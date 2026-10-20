//! The preview pane: the user's app in a frame (sized like a computer, tablet or
//! phone), point-and-select, and plain-language states while it builds or when
//! it fails.

use ak_types::preview::{PreviewStatus, ServerState};
use leptos::prelude::*;

use crate::backend::Backend;
use crate::chat::{Chat, send_message};
use crate::demo::{DEMO_PAGE, DEMO_URL};
use crate::i18n::t;
use crate::layout::{Device, Layout, fit_scale};
use crate::shell::change;

/// The demo page's URL (tests show the pane with it).
pub const DEMO_URL_FOR_TESTS: &str = DEMO_URL;

/// The open project's preview, shared with the app (which updates the status).
#[derive(Clone, Copy)]
pub struct Preview {
    pub status: RwSignal<PreviewStatus>,
    /// Point-and-select is on.
    pub picking: RwSignal<bool>,
}

impl Preview {
    pub fn new() -> Preview {
        Preview {
            status: RwSignal::new(PreviewStatus::default()),
            picking: RwSignal::new(false),
        }
    }
}

impl Default for Preview {
    fn default() -> Self {
        Preview::new()
    }
}

/// `scheme://host[:port]` of a URL (where point-and-select messages go).
pub fn origin_of(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let host = rest.split(['/', '?', '#']).next()?;
    (!scheme.is_empty() && !host.is_empty()).then(|| format!("{scheme}://{host}"))
}

/// What covers the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cover {
    /// Nothing: the page (if any) is usable.
    None,
    /// The preview is not running.
    Off,
    /// Starting or the first build: nothing to show yet.
    FirstBuild,
    /// A rebuild: the last good page stays, with a badge.
    Rebuilding,
    /// The screens or the backend failed; `output` for Details.
    Error { backend: bool, output: String },
}

pub fn cover(status: &PreviewStatus) -> Cover {
    if let ServerState::Error { output } = &status.frontend {
        return Cover::Error {
            backend: false,
            output: output.clone(),
        };
    }
    if let ServerState::Error { output } = &status.backend {
        return Cover::Error {
            backend: true,
            output: output.clone(),
        };
    }
    let busy = |s: &ServerState| matches!(s, ServerState::Starting | ServerState::Building);
    match (&status.url, &status.frontend) {
        (None, ServerState::Off) => Cover::Off,
        (None, _) => Cover::FirstBuild,
        (Some(_), f) if busy(f) || busy(&status.backend) => Cover::Rebuilding,
        _ => Cover::None,
    }
}

const TOOL: &str = "rounded-md px-2.5 py-1 text-sm text-gray-700 hover:bg-gray-100 disabled:opacity-40 dark:text-gray-300 dark:hover:bg-gray-800";
const TOOL_ON: &str = "rounded-md bg-blue-600 px-2.5 py-1 text-sm text-white";

#[component]
pub fn PreviewPane(layout: RwSignal<Layout>) -> impl IntoView {
    let preview = expect_context::<Preview>();
    let chat = expect_context::<Chat>();
    let backend = expect_context::<Backend>();
    let reload = RwSignal::new(0u32);
    let opening = RwSignal::new(false);
    let stage_size = RwSignal::new((0.0f64, 0.0f64));
    let stage = NodeRef::<leptos::html::Div>::new();
    let frame = NodeRef::<leptos::html::Iframe>::new();
    let url = move || preview.status.with(|s| s.url.clone());
    let device = move || layout.with(|l| l.device);

    #[cfg(target_arch = "wasm32")]
    browser::wire(preview, chat, frame, stage, stage_size, url);

    let run_app = move |_| {
        opening.set(true);
        let backend = backend.clone();
        leptos::task::spawn_local(async move {
            let _ = backend.run_app().await;
            crate::backend::sleep(4_000).await;
            opening.set(false);
        });
    };
    let device_button = move |d: Device| {
        let on = move || device() == d;
        view! {
            <button
                type="button"
                data-device=d.key()
                aria-pressed=move || on().to_string()
                class=move || if on() { TOOL_ON } else { TOOL }
                on:click=move |_| change(layout, |l| l.device = d)
            >
                {t(match d {
                    Device::Computer => "preview-device-computer",
                    Device::Tablet => "preview-device-tablet",
                    Device::Phone => "preview-device-phone",
                })}
            </button>
        }
    };
    let devices = Device::ALL.map(device_button);

    // The frame: the whole stage on a computer, else the device's size scaled to fit.
    let frame_style = move || match device().size() {
        None => "position:absolute;inset:0;width:100%;height:100%".to_owned(),
        Some((w, h)) => {
            let s = fit_scale((w, h), stage_size.get());
            format!(
                "position:absolute;left:50%;top:12px;width:{w}px;height:{h}px;transform:translateX(-50%) scale({s});transform-origin:top center"
            )
        }
    };
    let framed = move || device().size().is_some();
    let page = move || {
        reload.track();
        url().map(|u| {
            let demo = u == DEMO_URL;
            view! {
                <iframe
                    node_ref=frame
                    data-preview="page"
                    title=t("preview-frame-title")
                    src=(!demo).then(|| u.clone())
                    srcdoc=demo.then_some(DEMO_PAGE)
                    class="h-full w-full border-0 bg-white"
                ></iframe>
            }
        })
    };
    let highlight = move || {
        chat.selection.get().map(|p| {
            let r = p.rect;
            view! {
                <div
                    data-highlight=""
                    aria-hidden="true"
                    class="pointer-events-none absolute rounded-sm outline-2 outline-offset-2 outline-blue-500"
                    style=format!("left:{}px;top:{}px;width:{}px;height:{}px", r.x, r.y, r.width, r.height)
                ></div>
            }
        })
    };

    view! {
        <div data-pane="preview" class="flex min-h-0 flex-1 flex-col">
            <div class="flex shrink-0 flex-wrap items-center gap-1 border-b border-gray-200 bg-white px-2 py-1 dark:border-gray-800 dark:bg-gray-950">
                <div role="group" aria-label=t("preview-device") class="flex gap-0.5">{devices}</div>
                <span class="mx-1 h-5 w-px bg-gray-200 dark:bg-gray-700"></span>
                <button
                    type="button"
                    data-action="pick"
                    aria-pressed=move || preview.picking.get().to_string()
                    title=t("preview-pick-hint")
                    disabled=move || url().is_none()
                    class=move || if preview.picking.get() { TOOL_ON } else { TOOL }
                    on:click=move |_| preview.picking.update(|p| *p = !*p)
                >
                    {t("preview-pick")}
                </button>
                <button
                    type="button"
                    data-action="reload"
                    disabled=move || url().is_none()
                    class=TOOL
                    on:click=move |_| reload.update(|n| *n += 1)
                >
                    {t("preview-reload")}
                </button>
                <span class="flex-1"></span>
                <Show when=move || opening.get()>
                    <span class="text-xs text-gray-500 dark:text-gray-400">{t("preview-opening-window")}</span>
                </Show>
                <button
                    type="button"
                    data-action="run-app"
                    title=t("preview-run-app-hint")
                    disabled=move || !preview.status.with(PreviewStatus::ready) || opening.get()
                    class=TOOL
                    on:click=run_app
                >
                    {t("preview-run-app")}
                </button>
            </div>
            <Show when=move || preview.picking.get()>
                <p data-notice="picking" class="shrink-0 bg-blue-50 px-3 py-1.5 text-sm text-blue-900 dark:bg-blue-950 dark:text-blue-100">
                    {t("preview-picking")}
                </p>
            </Show>
            <div node_ref=stage data-preview="stage" class="relative min-h-0 flex-1 overflow-hidden">
                <div
                    data-preview="frame"
                    data-device=move || device().key()
                    style=frame_style
                    class=move || {
                        if framed() {
                            "overflow-hidden rounded-[1.5rem] border-8 border-gray-800 bg-white shadow-lg"
                        } else {
                            ""
                        }
                    }
                >
                    {page}
                    {highlight}
                </div>
                <CoverView chat />
            </div>
        </div>
    }
}

/// The plain-language layer over the page.
#[component]
fn CoverView(chat: Chat) -> impl IntoView {
    let preview = expect_context::<Preview>();
    let state = Memo::new(move |_| preview.status.with(cover));
    move || {
        match state.get() {
        Cover::None => ().into_any(),
        Cover::Off => view! {
            <div data-cover="off" class="absolute inset-0 flex items-center justify-center p-6 text-center">
                <div class="max-w-sm">
                    <p class="text-base font-medium">{t("shell-preview-empty")}</p>
                    <p class="mt-2 text-sm text-gray-500 dark:text-gray-400">{t("preview-off")}</p>
                </div>
            </div>
        }
        .into_any(),
        Cover::FirstBuild => view! {
            <div data-cover="first-build" role="status" class="absolute inset-0 flex items-center justify-center p-6 text-center">
                <div class="max-w-sm">
                    <p class="animate-pulse text-base font-medium">{t("preview-first-build")}</p>
                    <p class="mt-2 text-sm text-gray-500 dark:text-gray-400">{t("preview-first-build-hint")}</p>
                </div>
            </div>
        }
        .into_any(),
        Cover::Rebuilding => view! {
            <span data-cover="rebuilding" role="status" class="absolute top-3 right-3 animate-pulse rounded-full bg-gray-900/80 px-3 py-1 text-xs text-white">
                {t("preview-rebuilding")}
            </span>
        }
        .into_any(),
        Cover::Error { backend, output } => {
            let busy = move || chat.log.with(|l| l.running);
            let ask = move |_| send_message(chat, t("preview-fix-message")());
            view! {
                <div data-cover="error" role="alert" class="absolute inset-0 flex items-center justify-center bg-white/90 p-6 dark:bg-gray-950/90">
                    <div class="max-w-md rounded-xl border border-red-200 bg-white p-5 shadow-sm dark:border-red-900 dark:bg-gray-900">
                        <p class="font-medium">
                            {if backend { t("preview-error-backend")() } else { t("preview-error-screens")() }}
                        </p>
                        <p class="mt-2 text-sm text-gray-600 dark:text-gray-400">
                            {move || if busy() { t("preview-error-ai-working")() } else { t("preview-error-hint")() }}
                        </p>
                        <button
                            type="button"
                            data-action="fix"
                            disabled=busy
                            class="mt-4 rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 disabled:opacity-50"
                            on:click=ask
                        >
                            {t("preview-fix")}
                        </button>
                        <details class="mt-3 text-xs text-gray-500 dark:text-gray-400">
                            <summary class="cursor-pointer select-none">{t("common-details")}</summary>
                            <pre class="mt-1 max-h-60 overflow-auto rounded bg-gray-100 p-2 whitespace-pre-wrap dark:bg-gray-800">{output}</pre>
                        </details>
                    </div>
                </div>
            }
            .into_any()
        }
    }
    }
}

#[cfg(target_arch = "wasm32")]
mod browser {
    //! Talking to the page (postMessage) and measuring the stage.

    use ak_types::preview::{InspectorCommand, InspectorEvent};
    use leptos::prelude::*;
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;

    use super::{Preview, origin_of};
    use crate::chat::Chat;

    pub fn wire(
        preview: Preview,
        chat: Chat,
        frame: NodeRef<leptos::html::Iframe>,
        stage: NodeRef<leptos::html::Div>,
        stage_size: RwSignal<(f64, f64)>,
        url: impl Fn() -> Option<String> + Copy + Send + Sync + 'static,
    ) {
        let post = move |cmd: InspectorCommand| {
            let (Some(iframe), Some(origin)) =
                (frame.get_untracked(), url().and_then(|u| origin_of(&u)))
            else {
                return;
            };
            if let Some(w) = iframe.content_window() {
                let _ = w.post_message(&cmd.message().into(), &origin);
            }
        };
        // Turn the page's inspector on and off with the button.
        Effect::new(move |_| {
            let on = preview.picking.get();
            post(if on {
                InspectorCommand::Enable
            } else {
                InspectorCommand::Disable
            });
        });
        // Answers from the page: only from our frame, and from its origin.
        let _ = window_event_listener(leptos::ev::message, move |e| {
            let Some(iframe) = frame.get_untracked() else {
                return;
            };
            let ours = match (e.source(), iframe.content_window()) {
                (Some(source), Some(w)) => js_sys::Object::is(&source, &w),
                _ => false,
            };
            if !ours || url().and_then(|u| origin_of(&u)).as_deref() != Some(e.origin().as_str()) {
                return;
            }
            let Some(text) = e.data().as_string() else {
                return;
            };
            match InspectorEvent::parse(&text) {
                // The page (re)loaded: keep its inspector in step.
                Some(InspectorEvent::Ready) if preview.picking.get_untracked() => {
                    post(InspectorCommand::Enable);
                }
                Some(InspectorEvent::Select { target }) => {
                    chat.selection.set(Some(target));
                    preview.picking.set(false);
                }
                Some(InspectorEvent::Cancel) => preview.picking.set(false),
                _ => {}
            }
        });
        // Escape with the focus outside the frame also ends it.
        let _ = window_event_listener(leptos::ev::keydown, move |e| {
            if e.key() == "Escape" && preview.picking.get_untracked() {
                preview.picking.set(false);
            }
        });
        // The stage's size, for scaling device frames (it changes with the panes).
        Effect::new(move |_| {
            let Some(el) = stage.get() else { return };
            let measure = Closure::<dyn Fn()>::new(move || {
                if let Some(el) = stage.get_untracked() {
                    let el: &web_sys::Element = el.unchecked_ref();
                    stage_size.set((
                        f64::from(el.client_width()),
                        f64::from(el.client_height()) - 24.0,
                    ));
                }
            });
            if let Ok(observer) = web_sys::ResizeObserver::new(measure.as_ref().unchecked_ref()) {
                observer.observe(el.unchecked_ref());
                // Observes for the window's lifetime.
                std::mem::forget(observer);
            }
            measure.forget();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_come_from_the_page_url() {
        assert_eq!(
            origin_of("http://127.0.0.1:5173/?ak_bridge=ws%3A%2F%2F127.0.0.1%3A9").as_deref(),
            Some("http://127.0.0.1:5173")
        );
        assert_eq!(
            origin_of("https://x.test#a").as_deref(),
            Some("https://x.test")
        );
        assert_eq!(origin_of(DEMO_URL), None);
        assert_eq!(origin_of("nonsense"), None);
    }

    #[test]
    fn the_cover_follows_the_status() {
        let url = Some("http://127.0.0.1:1/".to_owned());
        let s = |frontend, backend, url: &Option<String>| PreviewStatus {
            frontend,
            backend,
            url: url.clone(),
        };
        assert_eq!(cover(&PreviewStatus::default()), Cover::Off);
        assert_eq!(
            cover(&s(ServerState::Building, ServerState::Starting, &None)),
            Cover::FirstBuild
        );
        assert_eq!(
            cover(&s(ServerState::Building, ServerState::Ready, &url)),
            Cover::Rebuilding
        );
        assert_eq!(
            cover(&s(ServerState::Ready, ServerState::Building, &url)),
            Cover::Rebuilding,
            "a backend restart"
        );
        assert_eq!(
            cover(&s(ServerState::Ready, ServerState::Ready, &url)),
            Cover::None
        );
        assert_eq!(
            cover(&s(ServerState::Ready, ServerState::Off, &url)),
            Cover::None
        );
        let broken = ServerState::Error {
            output: "error[E0308]".into(),
        };
        assert!(matches!(
            cover(&s(broken.clone(), ServerState::Ready, &url)),
            Cover::Error { backend: false, .. }
        ));
        assert!(matches!(
            cover(&s(ServerState::Ready, broken, &url)),
            Cover::Error { backend: true, .. }
        ));
    }
}
