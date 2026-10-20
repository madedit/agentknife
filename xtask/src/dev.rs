//! `cargo xtask dev`: runs AgentKnife (`cargo tauri dev` in crates/ak-app) with
//! AgentKnife's own toolchain, so nothing has to be installed or put on PATH
//! by hand. Arguments after `dev` go to `cargo tauri dev`.

use std::path::Path;
use std::process::Command;

use ak_doctor::{Pinned, Toolchain};

use crate::Result;

/// Inherited variables that would make cargo/rustup use another toolchain, or
/// break trunk (it reads `NO_COLOR` as a true/false flag).
const REMOVE: &[&str] = &["CARGO", "RUSTC", "RUSTDOC", "RUSTC_WRAPPER", "NO_COLOR"];

pub fn run(root: &Path, args: &[String]) -> Result<()> {
    let pinned = Pinned::builtin()?;
    let tc = Toolchain::new(Toolchain::default_root(), &pinned);
    let tauri = tc.exe(&tc.cargo_bin(), "cargo-tauri");
    if !tauri.exists() {
        return Err(format!(
            "{} is missing: install AgentKnife's toolchain first \
             (cargo run -p ak-doctor --example doctor_install)",
            tauri.display()
        )
        .into());
    }
    let mut path = tc.path_prefix();
    path.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let mut cmd = Command::new(&tauri);
    cmd.arg("tauri")
        .arg("dev")
        .args(args)
        .current_dir(root.join("crates/ak-app"))
        .envs(tc.env())
        .env("PATH", std::env::join_paths(&path)?)
        // trunk (started by tauri for ak-shell) must not look for updates.
        .env("TRUNK_SKIP_VERSION_CHECK", "true");
    for k in REMOVE {
        cmd.env_remove(k);
    }
    println!("==> cargo tauri dev (toolchain: {})", tc.root().display());
    let status = cmd.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("cargo tauri dev exited with {status}").into())
    }
}
