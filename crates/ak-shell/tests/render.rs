//! The window's panes per mode and language, rendered to HTML natively.
//! Dragging and saving are checked in a browser (`trunk serve`).

// Test code: clippy's allow-unwrap-in-tests does not cover helper fns here.
#![allow(clippy::unwrap_used)]

use ak_shell::layout::{CenterTab, Layout, Mode};
use ak_shell::{i18n, shell::Shell};
use ak_types::i18n::Locale;
use leptos::prelude::*;

fn render(locale: Locale, layout: Layout) -> String {
    let owner = Owner::new();
    owner.with(|| {
        i18n::provide(locale);
        view! { <Shell initial=layout /> }.into_view().to_html()
    })
}

fn pane(html: &str, name: &str) -> bool {
    html.contains(&format!(r#"data-pane="{name}""#))
}

#[test]
fn simple_mode_shows_only_the_preview_and_the_chat() {
    let html = render(Locale::En, Layout::default());
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
}

#[test]
fn full_mode_adds_files_timeline_and_the_code_tab() {
    let full = Layout {
        mode: Mode::Full,
        ..Layout::default()
    };
    let html = render(Locale::En, full);
    for shown in ["files", "file-list", "timeline", "preview", "chat"] {
        assert!(pane(&html, shown), "{shown} missing");
    }
    assert!(html.contains(r#"role="tablist""#));
    assert_eq!(html.matches(r#"role="separator""#).count(), 2);
    assert!(html.contains("min(260px, 30vw)"), "{html}");

    let code = render(
        Locale::En,
        Layout {
            tab: CenterTab::Code,
            ..full
        },
    );
    assert!(pane(&code, "code") && !pane(&code, "preview"));
    // The saved code tab does not leak into simple mode.
    let simple = render(
        Locale::En,
        Layout {
            tab: CenterTab::Code,
            ..Layout::default()
        },
    );
    assert!(pane(&simple, "preview") && !pane(&simple, "code"));
}

#[test]
fn all_text_follows_the_language() {
    let full = Layout {
        mode: Mode::Full,
        ..Layout::default()
    };
    let en = render(Locale::En, full);
    let zh = render(Locale::ZhTw, full);
    for (en_text, zh_text) in [
        ("Version history", "版本紀錄"),
        ("Chat with the AI", "與 AI 對話"),
        ("Simple", "簡單"),
        ("Describe what you want…", "說說你想做什麼…"),
    ] {
        assert!(en.contains(en_text), "{en_text}");
        assert!(zh.contains(zh_text), "{zh_text}");
        assert!(!zh.contains(en_text), "English `{en_text}` in zh-TW");
    }
    // No fluent id leaked through as text.
    assert!(!en.contains(">shell-") && !zh.contains(">shell-"));
}
