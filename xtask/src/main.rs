//! Dev tasks for the AgentKnife workspace. Run with `cargo xtask <command>`.

#![forbid(unsafe_code)]

mod bridge;
mod ci;
mod dev;
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
        Some("vendor-codemirror") => match std::env::args().nth(2).as_deref() {
            Some("index") => vendor::write_index(&workspace_root()),
            _ => vendor::vendor_codemirror(&workspace_root()),
        },
        Some("pin-downloads") => downloads::pin(&workspace_root()),
        Some("vendor-kit") => kit::vendor(&workspace_root()),
        Some("check-knowledge") => knowledge::check_build(&workspace_root()),
        Some("dev") => {
            let rest: Vec<String> = std::env::args().skip(2).collect();
            dev::run(&workspace_root(), &rest)
        }
        Some("build") => {
            let rest: Vec<String> = std::env::args().skip(2).collect();
            dev::build(&workspace_root(), &rest)
        }
        Some("e2e") => {
            let rest: Vec<String> = std::env::args().skip(2).collect();
            dev::e2e(&workspace_root(), &rest)
        }
        _ => {
            eprintln!(
                "usage: cargo xtask <gen-toolchain|ci|dev|build|e2e|vendor-codemirror|pin-downloads|vendor-kit|check-knowledge>"
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
