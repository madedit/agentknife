//! The tools check and packaging cards in the chat, rendered to HTML natively.

// Test code: clippy's allow-unwrap-in-tests does not cover helper fns here.
#![allow(clippy::unwrap_used)]
// Leptos views nest deeply in types.
#![recursion_limit = "256"]

use ak_shell::backend::Backend;
use ak_shell::chat::log::{BundleCard, ChatLog, DoctorCard, Item};
use ak_shell::chat::{Chat, ChatPane};
use ak_shell::i18n;
use ak_types::doctor::{
    CheckItem, CheckScope, CheckStatus, DoctorProgress, DoctorReport, GuideStep, InstallPhase,
    SystemAction, SystemGuide,
};
use ak_types::i18n::Locale;
use ak_types::ipc::{BundleFailure, BundleResult, DoctorCheck};
use leptos::prelude::*;

fn render(locale: Locale, items: Vec<Item>) -> String {
    let owner = Owner::new();
    owner.with(|| {
        i18n::provide(locale);
        provide_context(Backend::demo());
        provide_context(Chat::new(ChatLog {
            items,
            ..ChatLog::default()
        }));
        view! { <ChatPane /> }.into_view().to_html()
    })
}

fn item(id: &str, scope: CheckScope, status: CheckStatus) -> CheckItem {
    CheckItem {
        id: id.into(),
        scope,
        status,
        found: None,
        wanted: None,
        detail: None,
    }
}

fn step(message: &str, command: Option<&str>) -> GuideStep {
    GuideStep {
        message: message.into(),
        command: command.map(Into::into),
        link: None,
    }
}

fn missing() -> DoctorCheck {
    DoctorCheck {
        report: DoctorReport {
            items: vec![
                item("rust", CheckScope::Isolated, CheckStatus::Ok),
                item("trunk", CheckScope::Isolated, CheckStatus::Missing),
                item("webview2", CheckScope::System, CheckStatus::Missing),
                item("linux-webkit2gtk", CheckScope::System, CheckStatus::Missing),
            ],
        },
        guides: vec![
            SystemGuide {
                item: "webview2".into(),
                covers: vec!["webview2".into()],
                steps: vec![step("doctor-guide-webview2-what", None)],
                action: Some(SystemAction::WingetWebView2),
            },
            SystemGuide {
                item: "linux-packages".into(),
                covers: vec!["linux-webkit2gtk".into()],
                steps: vec![step(
                    "doctor-guide-linux-run",
                    Some("sudo apt install libwebkit2gtk-4.1-dev"),
                )],
                action: None,
            },
        ],
    }
}

#[test]
fn the_tools_card_lists_what_is_missing_and_how_to_get_it() {
    let html = render(
        Locale::En,
        vec![Item::Doctor(DoctorCard {
            check: Some(missing()),
            ..DoctorCard::default()
        })],
    );
    assert!(
        html.contains(r#"data-tool="rust" data-ok="true""#),
        "{html}"
    );
    assert!(html.contains(r#"data-tool="trunk" data-ok="false""#));
    assert!(html.contains("Trunk (builds the app"));
    assert!(html.contains(r#"data-action="install-tools""#));
    assert!(
        html.contains(r#"data-guide="webview2""#)
            && html.contains(r#"data-action="system-action""#)
    );
    assert!(html.contains(r#"data-command="""#) && html.contains("libwebkit2gtk-4.1-dev"));
    assert!(
        html.contains(r#"data-action="copy""#) && html.contains(r#"data-action="check-again""#)
    );
    assert!(!html.contains("data-ready"));
    // The chat header offers both actions.
    assert!(
        html.contains(r#"data-action="check-tools""#) && html.contains(r#"data-action="bundle""#)
    );
}

#[test]
fn installing_shows_progress_and_a_ready_check_says_so() {
    let busy = render(
        Locale::ZhTw,
        vec![Item::Doctor(DoctorCard {
            check: Some(missing()),
            busy: true,
            progress: Some(DoctorProgress {
                item: "trunk".into(),
                step: 1,
                total: 2,
                phase: InstallPhase::Download {
                    done: 4,
                    size: Some(8),
                },
            }),
            error: None,
        })],
    );
    assert!(
        busy.contains("data-progress") && busy.contains("width:50%"),
        "{busy}"
    );
    assert!(busy.contains("正在下載"), "{busy}");
    assert!(
        !busy.contains(r#"data-action="install-tools""#),
        "no second install while busy"
    );

    let mut all_ok = missing();
    all_ok.guides.clear();
    for i in &mut all_ok.report.items {
        i.status = CheckStatus::Ok;
    }
    let ready = render(
        Locale::En,
        vec![Item::Doctor(DoctorCard {
            check: Some(all_ok),
            ..DoctorCard::default()
        })],
    );
    assert!(ready.contains("data-ready") && ready.contains("Everything you need is installed."));
    assert!(!ready.contains(r#"data-action="install-tools""#));
}

#[test]
fn packaging_shows_progress_then_install_steps_or_the_failure() {
    let running = render(
        Locale::En,
        vec![Item::Bundle(BundleCard::Running {
            stage: Some("bundle-checking".into()),
            lines: vec!["Compiling app".into()],
        })],
    );
    assert!(running.contains("Checking that no preview-only parts are inside"));
    assert!(running.contains("Compiling app"));
    // No second packaging while one runs.
    let start = running.find(r#"data-action="bundle""#).unwrap();
    let tag = &running[start..start + running[start..].find('>').unwrap()];
    assert!(tag.contains(" disabled"), "{tag}");

    let done = render(
        Locale::ZhTw,
        vec![Item::Bundle(BundleCard::Done(BundleResult {
            path: "C:/p/My App_0.1.0_x64.zip".into(),
            file: "My App_0.1.0_x64.zip".into(),
            app: "My App".into(),
            os: "windows".into(),
            steps: vec![
                step("bundle-guide-win-run", None),
                step("bundle-guide-win-smartscreen", None),
            ],
        }))],
    );
    assert!(
        done.contains("App 打包好了：My App_0.1.0_x64.zip"),
        "{done}"
    );
    assert!(
        done.contains("按兩下「My App」"),
        "steps carry the app name"
    );
    assert!(done.contains(r#"data-action="reveal""#));
    assert!(done.contains(r#"data-os="macos""#) && done.contains(r#"data-os="linux""#));
    assert!(
        !done.contains(r#"data-os="windows""#),
        "not the system it was made on"
    );

    let failed = render(
        Locale::En,
        vec![Item::Bundle(BundleCard::Failed(BundleFailure {
            kind: "build-failed".into(),
            detail: "error[E0308]".into(),
        }))],
    );
    assert!(failed.contains("could not be built") && failed.contains("error[E0308]"));
    assert!(failed.contains(r#"data-action="fix-bundle""#));
}
