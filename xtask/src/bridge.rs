//! Release builds of generated apps carry no preview bridge: without the `dev`
//! feature, ak-bridge must not pull in its WebSocket server or client.

use std::path::Path;
use std::process::Command;

use crate::Result;

/// Crates that only the `dev` feature may bring in, per target.
const DEV_ONLY: &[(&str, &[&str])] = &[
    ("native", &["tokio-tungstenite", "tungstenite"]),
    ("wasm32-unknown-unknown", &["futures-channel"]),
];

fn tree(root: &Path, target: &str, dev: bool) -> Result<String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let mut cmd = Command::new(cargo);
    cmd.args([
        "tree",
        "-p",
        "ak-bridge",
        "-e",
        "normal",
        "--prefix",
        "none",
    ])
    .current_dir(root);
    if target != "native" {
        cmd.args(["--target", target]);
    }
    if dev {
        cmd.args(["--features", "dev"]);
    }
    let out = cmd.output()?;
    if !out.status.success() {
        return Err(format!(
            "cargo tree failed: {}",
            String::from_utf8_lossy(&out.stderr)
        )
        .into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn has(tree: &str, krate: &str) -> bool {
    tree.lines()
        .any(|l| l.split_whitespace().next() == Some(krate))
}

pub fn check(root: &Path) -> Result<()> {
    let mut errors = Vec::new();
    for (target, crates) in DEV_ONLY {
        let release = tree(root, target, false)?;
        let dev = tree(root, target, true)?;
        for krate in *crates {
            if has(&release, krate) {
                errors.push(format!(
                    "ak-bridge ({target}) pulls in {krate} without the `dev` feature"
                ));
            }
        }
        // Guard against the check going stale: `dev` really does use them.
        if !crates.iter().any(|k| has(&dev, k)) {
            errors.push(format!(
                "ak-bridge ({target}) with `dev` uses none of {crates:?}; update xtask/src/bridge.rs"
            ));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n").into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_names_are_matched_exactly() {
        let t = "ak-bridge v0.1.0 (C:\\x)\ntokio-tungstenite v0.30.0\ntokio v1.54.0\n";
        assert!(has(t, "tokio-tungstenite"));
        assert!(!has(t, "tungstenite"));
    }
}
