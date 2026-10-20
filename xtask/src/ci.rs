//! `cargo xtask ci`: the pre-commit gate (AGENTS.md §8).

use std::path::Path;
use std::process::Command;

use crate::{Result, bridge, downloads, i18n, kit, layering, nonode, stack, vendor, versions};

/// Crates that must also build for `wasm32-unknown-unknown`.
const WASM_CRATES: &[&str] = &[
    "ak-types",
    "ak-shell",
    "ak-bridge",
    "ak-ui",
    "ak-ui-gallery",
];

pub fn run(root: &Path) -> Result<()> {
    step("fmt", || {
        cargo(root, &["fmt", "--all", "--check"])?;
        // Templates are separate workspaces.
        for template in kit::TEMPLATES {
            let manifest = format!("{template}/Cargo.toml");
            cargo(
                root,
                &["fmt", "--manifest-path", &manifest, "--all", "--check"],
            )?;
        }
        Ok(())
    })?;
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
        )?;
        // The preview-only code of ak-bridge (off by default).
        cargo(
            root,
            &[
                "clippy",
                "-p",
                "ak-bridge",
                "--all-targets",
                "--features",
                "dev",
                "--",
                "-D",
                "warnings",
            ],
        )
    })?;
    step("test", || {
        cargo(root, &["test", "--workspace"])?;
        cargo(root, &["test", "-p", "ak-bridge", "--features", "dev"])
    })?;
    step("layering", || layering::check(root))?;
    step("versions", || versions::check(root))?;
    step("downloads", || downloads::check(root))?;
    step("bridge", || bridge::check(root))?;
    step("kit", || kit::check(root))?;
    step("stack", || stack::check(root))?;
    step("i18n", || i18n::check(root))?;
    step("vendor", || vendor::check(root))?;
    step("no-node", || nonode::check(root))?;
    step("wasm", || {
        let mut args = vec!["clippy", "--target", "wasm32-unknown-unknown"];
        for krate in WASM_CRATES {
            args.extend(["-p", krate]);
        }
        args.extend(["--", "-D", "warnings"]);
        cargo(root, &args)?;
        cargo(
            root,
            &[
                "clippy",
                "--target",
                "wasm32-unknown-unknown",
                "-p",
                "ak-bridge",
                "--features",
                "dev",
                "--",
                "-D",
                "warnings",
            ],
        )
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
