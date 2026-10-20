//! `cargo xtask dev` / `cargo xtask build`: `cargo tauri dev` or a release
//! `cargo tauri build --no-bundle` in crates/ak-app, with AgentKnife's own
//! toolchain, so nothing has to be installed or put on PATH by hand. Further
//! arguments go to `cargo tauri`.

use std::path::Path;
use std::process::Command;

use ak_doctor::{Pinned, Toolchain};

use crate::Result;

/// Inherited variables that would make cargo/rustup use another toolchain, or
/// break trunk (it reads `NO_COLOR` as a true/false flag).
const REMOVE: &[&str] = &["CARGO", "RUSTC", "RUSTDOC", "RUSTC_WRAPPER", "NO_COLOR"];

/// Runs AgentKnife with hot reload.
pub fn run(root: &Path, args: &[String]) -> Result<()> {
    tauri(root, &["dev"], args)
}

/// Builds `target/release/agentknife(.exe)` (the shell's screens built in).
pub fn build(root: &Path, args: &[String]) -> Result<()> {
    tauri(root, &["build", "--no-bundle"], args)?;
    let exe = root
        .join("target")
        .join("release")
        .join(format!("agentknife{}", std::env::consts::EXE_SUFFIX));
    println!("built {}", exe.display());
    Ok(())
}

fn tauri(root: &Path, command: &[&str], args: &[String]) -> Result<()> {
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
        .args(command)
        .args(args)
        .current_dir(root.join("crates/ak-app"))
        .envs(tc.env())
        .env("PATH", std::env::join_paths(&path)?)
        // trunk (started by tauri for ak-shell) must not look for updates.
        .env("TRUNK_SKIP_VERSION_CHECK", "true");
    for k in REMOVE {
        cmd.env_remove(k);
    }
    println!(
        "==> cargo tauri {} (toolchain: {})",
        command.join(" "),
        tc.root().display()
    );
    let status = cmd.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("cargo tauri {} exited with {status}", command.join(" ")).into())
    }
}
