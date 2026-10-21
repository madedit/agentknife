//! The Log tab, rendered to HTML natively.

#![allow(clippy::unwrap_used)]

use ak_shell::backend::Backend;
use ak_shell::i18n;
use ak_shell::logs::{LogPane, ProjectLog};
use ak_types::i18n::Locale;
use ak_types::log::{LogLine, LogSource};
use leptos::prelude::*;

fn render(locale: Locale, lines: Vec<LogLine>) -> String {
    let owner = Owner::new();
    owner.with(|| {
        i18n::provide(locale);
        provide_context(Backend::demo());
        let log = ProjectLog::new();
        log.append(lines);
        provide_context(log);
        view! { <LogPane /> }.into_view().to_html()
    })
}

fn line(seq: u64, source: LogSource, text: &str) -> LogLine {
    LogLine {
        seq,
        at_ms: 0,
        source,
        text: text.into(),
    }
}

#[test]
fn every_line_shows_with_its_source_and_errors_stand_out() {
    let html = render(
        Locale::ZhTw,
        vec![
            line(1, LogSource::Window, "   Compiling app v0.1.0"),
            line(2, LogSource::Window, "error: could not compile `app`"),
            line(3, LogSource::Ai, "> 讀取 main.rs"),
        ],
    );
    assert!(html.contains(r#"data-pane="log""#), "{html}");
    for seq in 1..=3 {
        assert!(html.contains(&format!(r#"data-log-line="{seq}""#)), "{seq}");
    }
    assert!(html.contains(r#"data-source="window""#) && html.contains(r#"data-source="ai""#));
    assert!(html.contains("真正的視窗") && html.contains("AI 步驟"));
    assert_eq!(html.matches("text-red-700").count(), 1, "{html}");
    // Filters: all plus each source; "all" is on.
    for key in ["all", "screens", "backend", "window", "bundle", "ai"] {
        assert!(
            html.contains(&format!(r#"data-log-filter="{key}""#)),
            "{key}"
        );
    }
    assert!(
        html.contains(r#"data-log-filter="all" aria-pressed="true""#),
        "{html}"
    );
    assert!(
        html.contains(r#"data-action="copy-log""#) && html.contains(r#"data-action="clear-log""#)
    );
    assert!(!html.contains("data-log-empty"));
}

#[test]
fn an_empty_log_says_what_will_appear() {
    let html = render(Locale::En, Vec::new());
    assert!(html.contains("data-log-empty"), "{html}");
    assert!(html.contains("Nothing has been printed yet"));
}
