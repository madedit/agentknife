//! Dev tasks for the AgentKnife workspace. Run with `cargo xtask <command>`.

#![forbid(unsafe_code)]

mod bridge;
mod ci;
mod downloads;
mod i18n;
mod kit;
mod knowledge;
mod layering;
mod nonode;
mod stack;
mod vendor;
mod versions;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() -> ExitCode {
    let cmd = std::env::args().nth(1);
    let result = match cmd.as_deref() {
        Some("gen-toolchain") => gen_toolchain(),
        Some("ci") => ci::run(&workspace_root()),
        Some("vendor-codemirror") => vendor::vendor_codemirror(&workspace_root()),
        Some("pin-downloads") => downloads::pin(&workspace_root()),
        Some("vendor-kit") => kit::vendor(&workspace_root()),
        Some("check-knowledge") => knowledge::check_build(&workspace_root()),
        _ => {
            eprintln!(
                "usage: cargo xtask <gen-toolchain|ci|vendor-codemirror|pin-downloads|vendor-kit|check-knowledge>"
            );
            return ExitCode::FAILURE;
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask: {e}");
            ExitCode::FAILURE
        }
    }
}

fn workspace_root() -> PathBuf {
    // xtask/ lives directly under the workspace root.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn gen_toolchain() -> Result<()> {
    let root = workspace_root();
    let versions = versions::Versions::load(&root)?;
    std::fs::write(
        root.join("rust-toolchain.toml"),
        versions::render_toolchain(&versions),
    )?;
    println!(
        "wrote rust-toolchain.toml (rust {})",
        versions.rust.toolchain
    );
    Ok(())
}
