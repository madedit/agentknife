//! The preview pane per status and device, rendered to HTML natively. Messages
//! with the page (point-and-select) are checked in a browser.

// Test code: clippy's allow-unwrap-in-tests does not cover helper fns here.
#![allow(clippy::unwrap_used)]
// Leptos views nest deeply in types.
#![recursion_limit = "256"]

use ak_shell::backend::Backend;
use ak_shell::chat::Chat;
use ak_shell::chat::log::ChatLog;
use ak_shell::i18n;
use ak_shell::layout::{Device, Layout};
use ak_shell::logs::ProjectLog;
use ak_shell::preview::{Preview, PreviewPane};
use ak_types::i18n::Locale;
use ak_types::log::{BuildProgress, LogLine, LogSource};
use ak_types::preview::{Picked, PreviewStatus, Rect, ServerState, Size};
use leptos::prelude::*;

const URL: &str = "http://127.0.0.1:5173/?ak_bridge=x";

fn render(locale: Locale, status: PreviewStatus, device: Device, picked: Option<Picked>) -> String {
    let owner = Owner::new();
    owner.with(|| {
        i18n::provide(locale);
        provide_context(Backend::demo());
        let chat = Chat::new(ChatLog::default());
        chat.selection.set(picked);
        provide_context(chat);
        let preview = Preview::new();
        preview.status.set(status);
        provide_context(preview);
        let log = ProjectLog::new();
        log.append(
            [
                (LogSource::Screens, "Compiling serde v1.0.0"),
                (LogSource::Ai, "> Writing main.rs"),
            ]
            .into_iter()
            .zip(1..)
            .map(|((source, text), seq)| LogLine {
                seq,
                at_ms: 0,
                source,
                text: text.into(),
                progress: None,
            })
            .chain([LogLine {
                seq: 2,
                at_ms: 0,
                source: LogSource::Screens,
                text: "tauri".into(),
                progress: Some(BuildProgress {
                    done: 137,
                    total: 274,
                }),
            }])
            .collect(),
        );
        provide_context(log);
        let layout = RwSignal::new(Layout {
            device,
            ..Layout::default()
        });
        view! { <PreviewPane layout /> }.into_view().to_html()
    })
}

fn status(frontend: ServerState, url: Option<&str>) -> PreviewStatus {
    PreviewStatus {
        frontend,
        backend: ServerState::Ready,
        url: url.map(Into::into),
    }
}

#[test]
fn a_ready_preview_shows_the_page_and_its_tools() {
    let html = render(
        Locale::En,
        status(ServerState::Ready, Some(URL)),
        Device::Computer,
        None,
    );
    assert!(
        html.contains(r#"data-preview="page""#) && html.contains(URL),
        "{html}"
    );
    assert!(!html.contains("data-cover"));
    for action in ["pick", "reload", "run-app"] {
        assert!(
            html.contains(&format!(r#"data-action="{action}""#)),
            "{action}"
        );
    }
    assert!(
        html.contains(r#"data-device="computer" aria-pressed="true""#),
        "{html}"
    );
    assert!(html.contains("inset:0"));
}

#[test]
fn device_frames_take_the_device_size() {
    let html = render(
        Locale::En,
        status(ServerState::Ready, Some(URL)),
        Device::Phone,
        None,
    );
    assert!(html.contains("width:390px;height:844px"), "{html}");
    // `AK_SHELL_DUMP_PREVIEW=<file>`: the demo page in a phone frame while it
    // rebuilds, and an error, to look at with the real CSS.
    if let Some(path) = std::env::var_os("AK_SHELL_DUMP_PREVIEW") {
        let demo = ak_shell::preview::DEMO_URL_FOR_TESTS;
        let phone = render(
            Locale::ZhTw,
            status(ServerState::Building, Some(demo)),
            Device::Phone,
            None,
        );
        let error = render(
            Locale::ZhTw,
            status(
                ServerState::Error {
                    output: "error[E0308]: mismatched types".into(),
                },
                Some(demo),
            ),
            Device::Computer,
            None,
        );
        std::fs::write(path, format!("{phone}<hr>{error}")).unwrap();
    }
    assert!(html.contains(r#"data-device="phone" aria-pressed="true""#));
    assert!(html.contains("border-8"), "phone frame");
}

#[test]
fn building_and_errors_are_explained() {
    let first = render(
        Locale::ZhTw,
        status(ServerState::Building, None),
        Device::Computer,
        None,
    );
    assert!(first.contains(r#"data-cover="first-build""#) && first.contains("正在準備預覽"));
    assert!(!first.contains(r#"data-preview="page""#));
    assert!(first.contains(r#"data-action="pick""#) && first.contains("disabled"));

    let again = render(
        Locale::En,
        status(ServerState::Building, Some(URL)),
        Device::Computer,
        None,
    );
    assert!(
        again.contains(r#"data-cover="rebuilding""#) && again.contains(r#"data-preview="page""#)
    );

    let broken = render(
        Locale::En,
        status(
            ServerState::Error {
                output: "error[E0308]: mismatched types".into(),
            },
            Some(URL),
        ),
        Device::Computer,
        None,
    );
    assert!(broken.contains(r#"data-cover="error""#) && broken.contains(r#"data-action="fix""#));
    assert!(broken.contains("screens cannot be built") && broken.contains("error[E0308]"));

    let off = render(Locale::En, PreviewStatus::default(), Device::Computer, None);
    assert!(off.contains(r#"data-cover="off""#));
}

#[test]
fn the_picked_element_is_outlined() {
    let picked = Picked {
        ak_id: Some("greet-button".into()),
        exact: true,
        path: "main > button".into(),
        tag: "button".into(),
        text: Some("Greet".into()),
        rect: Rect {
            x: 10.0,
            y: 20.0,
            width: 80.0,
            height: 30.0,
        },
        viewport: Size {
            width: 800.0,
            height: 600.0,
        },
        canvas: None,
    };
    let html = render(
        Locale::En,
        status(ServerState::Ready, Some(URL)),
        Device::Computer,
        Some(picked),
    );
    assert!(html.contains("data-highlight"));
    assert!(
        html.contains("left:10px;top:20px;width:80px;height:30px"),
        "{html}"
    );
}

#[test]
fn a_build_shows_its_log_in_a_window_over_the_preview() {
    let first = render(
        Locale::ZhTw,
        status(ServerState::Building, None),
        Device::Computer,
        None,
    );
    assert!(first.contains(r#"data-window="build-log""#), "{first}");
    assert!(first.contains("Compiling serde"), "the build's lines");
    assert!(!first.contains("Writing main.rs"), "not the AI's steps");
    assert!(first.contains(r#"data-action="toggle-build-log""#) && first.contains("縮小"));
    // cargo's counts as a percentage and a bar.
    assert!(first.contains(r#"data-build-percent="50""#) && first.contains(">50%<"));
    assert!(first.contains(r#"data-build-part="screens""#) && first.contains("畫面"));
    assert!(first.contains("width:50%"));
    assert!(
        !first.contains(">tauri<"),
        "a progress report is not a line"
    );
    let rebuild = render(
        Locale::En,
        status(ServerState::Building, Some(URL)),
        Device::Computer,
        None,
    );
    assert!(rebuild.contains(r#"data-window="build-log""#) && rebuild.contains("Updating"));
    let ready = render(
        Locale::En,
        status(ServerState::Ready, Some(URL)),
        Device::Computer,
        None,
    );
    assert!(!ready.contains("build-log"), "{ready}");
}
