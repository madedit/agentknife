//! The screens per state, mode and language, rendered to HTML natively.
//! Clicking, dragging and saving are checked in a browser (`trunk serve`).

// Test code: clippy's allow-unwrap-in-tests does not cover helper fns here.
#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};

use ak_shell::app::{AppView, Screen};
use ak_shell::backend::{Backend, CallError, Demo};
use ak_shell::connect::{ConnectWizard, Wizard, error_ids};
use ak_shell::i18n;
use ak_shell::layout::{CenterTab, Layout, Mode};
use ak_types::i18n::{I18n, Locale};
use ak_types::ipc::{
    ConnectError, HomeState, ModelCheck, ModelChoice, PRESETS, ProjectRow, preset,
};
use ak_types::settings::AiConnection;
use leptos::prelude::*;

fn with_context(locale: Locale, f: impl FnOnce() -> String) -> String {
    let owner = Owner::new();
    owner.with(|| {
        i18n::provide(locale);
        provide_context(Backend::Demo(Arc::new(Mutex::new(Demo::new()))));
        f()
    })
}

fn home_state() -> HomeState {
    HomeState {
        projects: vec![ProjectRow {
            id: "p1".into(),
            name: "Shopping List".into(),
            path: "C:/Users/me/AgentKnife/Projects/Shopping List".into(),
            last_opened_at: 0,
            exists: true,
        }],
        ai: Some(AiConnection {
            provider: "anthropic".into(),
            base_url: None,
            model: "claude-opus-5-5".into(),
            effort: None,
        }),
        projects_folder: "C:/Users/me/AgentKnife/Projects".into(),
    }
}

fn render(locale: Locale, screen: Screen, layout: Layout) -> String {
    with_context(locale, || {
        let screen = RwSignal::new(screen);
        let home = RwSignal::new(home_state());
        view! { <AppView screen home layout /> }
            .into_view()
            .to_html()
    })
}

fn workspace(locale: Locale, layout: Layout) -> String {
    let row = home_state().projects.remove(0);
    render(locale, Screen::Workspace(row), layout)
}

fn wizard(locale: Locale, w: Wizard) -> String {
    with_context(locale, || {
        view! { <ConnectWizard initial=w on_done=Callback::new(|_| {}) /> }
            .into_view()
            .to_html()
    })
}

fn pane(html: &str, name: &str) -> bool {
    html.contains(&format!(r#"data-pane="{name}""#))
}

#[test]
fn simple_mode_shows_only_the_preview_and_the_chat() {
    let html = workspace(Locale::En, Layout::default());
    assert!(pane(&html, "preview") && pane(&html, "chat"), "{html}");
    for hidden in ["files", "timeline", "code"] {
        assert!(!pane(&html, hidden), "{hidden} shown in simple mode");
    }
    assert!(!html.contains(r#"role="tablist""#));
    // One splitter: the chat's.
    assert_eq!(html.matches(r#"role="separator""#).count(), 1);
    assert!(html.contains(r#"aria-label="Resize the chat panel""#));
    assert!(
        html.contains(r#"data-mode="simple" aria-pressed="true""#),
        "{html}"
    );
    // The project's name and the way home are in the top bar.
    assert!(html.contains("Shopping List") && html.contains(r#"data-action="home""#));
    assert!(
        html.contains("Claude (Anthropic) · claude-opus-5-5"),
        "{html}"
    );
}

#[test]
fn full_mode_adds_files_timeline_and_the_code_tab() {
    let full = Layout {
        mode: Mode::Full,
        ..Layout::default()
    };
    let html = workspace(Locale::En, full);
    for shown in ["files", "file-list", "timeline", "preview", "chat"] {
        assert!(pane(&html, shown), "{shown} missing");
    }
    assert!(html.contains(r#"role="tablist""#));
    assert_eq!(html.matches(r#"role="separator""#).count(), 2);
    assert!(html.contains("min(260px, 30vw)"), "{html}");

    let code = workspace(
        Locale::En,
        Layout {
            tab: CenterTab::Code,
            ..full
        },
    );
    assert!(pane(&code, "code") && !pane(&code, "preview"));
    // The saved code tab does not leak into simple mode.
    let simple = workspace(
        Locale::En,
        Layout {
            tab: CenterTab::Code,
            ..Layout::default()
        },
    );
    assert!(pane(&simple, "preview") && !pane(&simple, "code"));
}

#[test]
fn home_lists_projects_and_offers_new_and_open() {
    let html = render(Locale::En, Screen::Home, Layout::default());
    assert!(html.contains(r#"data-screen="home""#));
    assert!(html.contains(r#"data-project="p1""#) && html.contains("Shopping List"));
    assert!(html.contains(r#"data-action="new""#) && html.contains(r#"data-action="open-folder""#));
    // No mode switch outside a project.
    assert!(!html.contains("data-mode="));
    // Opened long ago (unix 0).
    assert!(html.contains(" days ago"), "{html}");
    assert!(html.contains(r#"data-action="change-ai""#));

    // The first-run wizard has no way around it.
    let first = render(
        Locale::En,
        Screen::Connect { back: false },
        Layout::default(),
    );
    assert!(first.contains(r#"data-screen="connect""#));
    assert!(!first.contains(r#"data-action="change-ai""#) && !first.contains(">Cancel<"));
}

#[test]
fn the_wizard_offers_cloud_and_local_choices() {
    let html = wizard(Locale::En, Wizard::default());
    for p in PRESETS {
        assert!(
            html.contains(&format!(r#"data-preset="{}""#, p.id)),
            "{}",
            p.id
        );
    }
    let zh = wizard(Locale::ZhTw, Wizard::default());
    assert!(zh.contains("網路上的服務") && zh.contains("這台電腦上"));

    let mut w = Wizard::default();
    w.choose(preset("anthropic").unwrap());
    let html = wizard(Locale::En, w.clone());
    assert!(html.contains(r#"type="password""#) && !html.contains(r#"type="url""#));
    assert!(html.contains("console.anthropic.com"));
    assert!(html.contains(r#"data-action="test""#) && html.contains("disabled"));

    w.models_loaded(Err(CallError::Connect(ConnectError::Unreachable {
        detail: "connection refused".into(),
    })));
    let html = wizard(Locale::En, w);
    assert!(html.contains(r#"data-error="connect-error-unreachable""#));
    assert!(html.contains("connection refused"), "details shown");

    let mut local = Wizard::default();
    local.choose(preset("ollama").unwrap());
    let html = wizard(Locale::En, local);
    assert!(html.contains(r#"type="url""#) && !html.contains(r#"type="password""#));
    assert!(html.contains("Make sure Ollama is running"), "{html}");
}

#[test]
fn a_model_without_tools_gets_a_warning() {
    let mut w = Wizard::default();
    w.choose(preset("ollama").unwrap());
    w.models_loaded(Ok(vec![ModelChoice {
        id: "llama3.2:3b".into(),
        display_name: None,
    }]));
    w.checked(Ok(ModelCheck::default()));
    let html = wizard(Locale::En, w.clone());
    assert!(
        html.contains(r#"data-check="no-tools""#) && html.contains(r#"data-check="no-vision""#)
    );
    assert!(html.contains(r#"data-action="save""#));

    w.checked(Ok(ModelCheck {
        tool_calling: true,
        vision: true,
    }));
    let html = wizard(Locale::En, w);
    assert!(html.contains(r#"data-check="ok""#) && !html.contains("no-vision"));
}

#[test]
fn every_text_exists_in_every_language() {
    let mut ids = error_ids();
    for p in PRESETS {
        ids.push(format!("connect-preset-{}", p.id));
        ids.push(format!("connect-preset-{}-about", p.id));
    }
    for locale in Locale::ALL {
        let i = I18n::new(locale).unwrap();
        for id in &ids {
            assert_ne!(&i.t(id), id, "{id} missing in {locale}");
        }
    }
    // And nothing English leaks into zh-TW screens.
    let full = Layout {
        mode: Mode::Full,
        ..Layout::default()
    };
    let zh = workspace(Locale::ZhTw, full) + &render(Locale::ZhTw, Screen::Home, full);
    for en in [
        "Version history",
        "Chat with the AI",
        "Your projects",
        "New project",
    ] {
        assert!(!zh.contains(en), "English `{en}` in zh-TW");
    }
    assert!(!zh.contains(">shell-") && !zh.contains(">home-") && !zh.contains(">connect-"));
}
