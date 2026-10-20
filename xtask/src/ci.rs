//! `cargo xtask ci`: the pre-commit gate (AGENTS.md §8).

use std::path::Path;
use std::process::Command;

use crate::{Result, i18n, layering, nonode, vendor, versions};

/// Crates that must also build for `wasm32-unknown-unknown`.
const WASM_CRATES: &[&str] = &["ak-types", "ak-shell"];

pub fn run(root: &Path) -> Result<()> {
    step("fmt", || cargo(root, &["fmt", "--all", "--check"]))?;
    step("clippy", || {
        cargo(
            root,
            &[
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
        )
    })?;
    step("test", || cargo(root, &["test", "--workspace"]))?;
    step("layering", || layering::check(root))?;
    step("versions", || versions::check(root))?;
    step("i18n", || i18n::check(root))?;
    step("vendor", || vendor::check(root))?;
    step("no-node", || nonode::check(root))?;
    step("wasm", || {
        let mut args = vec!["build", "--target", "wasm32-unknown-unknown"];
        for krate in WASM_CRATES {
            args.extend(["-p", krate]);
        }
        cargo(root, &args)
    })?;
    println!("xtask ci: all checks passed");
    Ok(())
}

fn step(name: &str, f: impl FnOnce() -> Result<()>) -> Result<()> {
    println!("==> {name}");
    f().map_err(|e| format!("check `{name}` failed: {e}").into())
}

fn cargo(root: &Path, args: &[&str]) -> Result<()> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let status = Command::new(cargo).args(args).current_dir(root).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("`cargo {}` exited with {status}", args.join(" ")).into())
    }
}
