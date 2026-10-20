//! Installs the isolated toolchain on this computer (downloads several hundred MB
//! into the toolchain folder; nothing outside it changes).
//!
//! ```text
//! cargo run -p ak-doctor --example doctor_install
//! # AK_TOOLCHAIN_ROOT=… to install somewhere else
//! ```

use std::error::Error;
use std::time::Duration;

use ak_doctor::fetch::HttpFetch;
use ak_doctor::probe::SystemProbe;
use ak_doctor::{Installer, Pinned, Platform, Toolchain, assets};
use ak_types::doctor::InstallPhase;
use ak_types::i18n::{FluentArgs, I18n, Locale};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let pinned = Pinned::builtin()?;
    let root = std::env::var("AK_TOOLCHAIN_ROOT")
        .map(Into::into)
        .unwrap_or_else(|_| Toolchain::default_root());
    let toolchain = Toolchain::new(root, &pinned);
    println!("toolchain folder: {}", toolchain.root().display());
    let i18n = I18n::new(Locale::ZhTw)?;
    let installer = Installer {
        toolchain: &toolchain,
        pinned: &pinned,
        platform: Platform::current(),
        probe: &SystemProbe::default(),
        runner: &SystemProbe {
            timeout: Duration::from_secs(2 * 60 * 60),
        },
        fetch: &HttpFetch::new()?,
        target: assets::host_target(),
    };
    let report = installer
        .install(&|p| {
            // Download progress only every 10 MB, to keep the output short.
            if let InstallPhase::Download { done, size } = p.phase
                && done % (10 << 20) > (512 << 10)
                && Some(done) != size
            {
                return;
            }
            let mut args = FluentArgs::new();
            args.set("tool", i18n.t(&format!("doctor-item-{}", p.item)));
            args.set("step", p.step);
            args.set("total", p.total);
            let label = i18n.t_args(&format!("doctor-install-{}", p.phase.key()), &args);
            match p.phase {
                InstallPhase::Download { done, size } => println!(
                    "{label} {} / {} MB",
                    done >> 20,
                    size.map_or("?".into(), |s| (s >> 20).to_string())
                ),
                _ => println!("{label}"),
            }
        })
        .await;
    for item in &report.items {
        println!(
            "{:<16} {:?} found={} {}",
            item.id,
            item.status,
            item.found.as_deref().unwrap_or("-"),
            item.detail.as_deref().unwrap_or("")
        );
    }
    println!("ready: {}", report.ready());
    Ok(())
}
