//! Prints the doctor report for this computer and the guides for missing
//! system dependencies (read-only; installs nothing).
//!
//! ```text
//! cargo run -p ak-doctor --example doctor_report
//! # AK_TOOLCHAIN_ROOT=… to look at another toolchain folder
//! ```

use std::error::Error;

use ak_doctor::probe::SystemProbe;
use ak_doctor::system::guides;
use ak_doctor::{Pinned, Platform, Toolchain, check};
use ak_types::i18n::{I18n, Locale};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let pinned = Pinned::builtin()?;
    let root = std::env::var("AK_TOOLCHAIN_ROOT")
        .map(Into::into)
        .unwrap_or_else(|_| Toolchain::default_root());
    let toolchain = Toolchain::new(root, &pinned);
    println!("toolchain folder: {}", toolchain.root().display());
    let report = check(
        &toolchain,
        &pinned,
        Platform::current(),
        &SystemProbe::default(),
    )
    .await;
    let i18n = I18n::new(Locale::ZhTw)?;
    for item in &report.items {
        let status = format!("{:?}", item.status)
            .to_ascii_lowercase()
            .replace("wrongversion", "wrong-version");
        println!(
            "{:<34} {:<12} found={:<16} wanted={:<10} {}",
            i18n.t(&format!("doctor-item-{}", item.id)),
            i18n.t(&format!("doctor-status-{status}")),
            item.found.as_deref().unwrap_or("-"),
            item.wanted.as_deref().unwrap_or("-"),
            item.detail.as_deref().unwrap_or("")
        );
    }
    println!("ready: {}", report.ready());
    for guide in guides(&report, Platform::current(), &SystemProbe::default()).await {
        println!("\n## {}", i18n.t(&format!("doctor-item-{}", guide.item)));
        for (n, step) in (1..).zip(&guide.steps) {
            println!("{n}. {}", i18n.t(&step.message));
            for extra in [&step.command, &step.link].into_iter().flatten() {
                println!("   {extra}");
            }
        }
        if let Some(action) = &guide.action {
            println!(
                "[{}] {}",
                i18n.t(&format!("doctor-action-{}", action.key())),
                i18n.t(&format!("doctor-confirm-{}", action.key()))
            );
        }
    }
    Ok(())
}
