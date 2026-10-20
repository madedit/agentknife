//! The settings page for given values, rendered to HTML natively.

// Test code: clippy's allow-unwrap-in-tests does not cover helper fns here.
#![allow(clippy::unwrap_used)]
// Leptos views nest deeply in types.
#![recursion_limit = "256"]

use ak_shell::backend::Backend;
use ak_shell::i18n;
use ak_shell::settings::SettingsScreen;
use ak_types::i18n::Locale;
use ak_types::ipc::{CrashReport, CrashReports};
use ak_types::settings::{AiConnection, AppSettings, Permissions, Plans};
use leptos::prelude::*;

fn render(locale: Locale, s: AppSettings, crash: CrashReports) -> String {
    let owner = Owner::new();
    owner.with(|| {
        i18n::provide(locale);
        provide_context(Backend::demo());
        let ai = Signal::derive(|| {
            Some(AiConnection {
                provider: "ollama".into(),
                base_url: None,
                model: "qwen3:8b".into(),
                effort: None,
            })
        });
        view! {
            <SettingsScreen
                ai
                change_ai=Callback::new(|()| {})
                on_close=Callback::new(|()| {})
                initial=s
                crash=crash
            />
        }
        .into_view()
        .to_html()
    })
}

#[test]
fn every_setting_shows_its_current_value() {
    let s = AppSettings {
        language: Some(Locale::ZhTw),
        permissions: Permissions::AskEveryChange,
        plans: Plans::Never,
        max_fix_attempts: 5,
        keep_turns: 120,
        daily_cap_usd: Some(2.5),
        ..AppSettings::default()
    };
    let html = render(Locale::En, s, CrashReports::default());
    assert!(html.contains(r#"<option value="zh-TW" selected"#), "{html}");
    assert!(html.contains("繁體中文") && html.contains("Same as the computer"));
    assert!(html.contains("Ollama · qwen3:8b"));
    for checked in ["settings-permissions-ask", "settings-plans-never"] {
        let at = html.find(&format!(r#"data-choice="{checked}""#)).unwrap();
        let tag = &html[html[..at].rfind('<').unwrap()..at + html[at..].find('>').unwrap()];
        assert!(tag.contains("checked"), "{checked}: {tag}");
    }
    assert!(
        html.contains(r#"value="5""#)
            && html.contains(r#"value="120""#)
            && html.contains(r#"value="2.5""#)
    );
    assert!(html.contains("collects no usage data"));
    assert!(html.contains(r#"data-crash-none"#));
}

#[test]
fn crash_reports_can_be_copied_and_the_page_follows_the_language() {
    let crash = CrashReports {
        folder: "C:/x/crash-reports".into(),
        reports: vec![CrashReport {
            path: "C:/x/crash-reports/1.txt".into(),
            created_at: 0,
            text: "AgentKnife 0.1.0 crash report".into(),
        }],
    };
    let html = render(Locale::ZhTw, AppSettings::default(), crash);
    assert!(
        html.contains(r#"data-action="copy-crash""#)
            && html.contains(r#"data-action="show-crash-folder""#)
    );
    assert!(html.contains("當機報告") && html.contains("不收集任何使用資料"));
    assert!(
        html.contains(r#"<option value="" selected"#),
        "system language by default"
    );
    assert!(!html.contains(">settings-"), "raw fluent id in {html}");
}
