//! Crate layering rules (AGENTS.md §3), checked against `cargo metadata`.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use serde::Deserialize;

use crate::Result;

/// Core crates: plain Rust APIs, adapted to Tauri only by `ak-app`.
const CORE: &[&str] = &[
    "ak-agent",
    "ak-providers",
    "ak-doctor",
    "ak-project",
    "ak-preview",
    "ak-bundle",
    "ak-mcp",
];
/// Crates shipped inside generated apps; independent of AgentKnife internals.
const GENERATED_APP: &[&str] = &["ak-ui", "ak-bridge"];
/// Crates allowed to depend on tauri. `ak-bridge` ships inside generated Tauri apps.
const TAURI_ALLOWED: &[&str] = &["ak-app", "ak-bridge"];
/// Crates allowed to depend on anyhow (AGENTS.md §2).
const ANYHOW_ALLOWED: &[&str] = &["ak-app", "xtask"];

/// Package name -> names of its normal and build dependencies (dev-deps excluded).
pub type DepGraph = BTreeMap<String, Vec<String>>;

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
}

#[derive(Deserialize)]
struct Package {
    name: String,
    dependencies: Vec<Dependency>,
}

#[derive(Deserialize)]
struct Dependency {
    name: String,
    kind: Option<String>,
}

pub fn check(root: &Path) -> Result<()> {
    let violations = violations(&load(root)?);
    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations.join("\n").into())
    }
}

fn load(root: &Path) -> Result<DepGraph> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let out = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(root)
        .output()?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned().into());
    }
    let meta: Metadata = serde_json::from_slice(&out.stdout)?;
    Ok(meta
        .packages
        .into_iter()
        .map(|p| {
            let deps = p
                .dependencies
                .into_iter()
                .filter(|d| d.kind.as_deref() != Some("dev"))
                .map(|d| d.name)
                .collect();
            (p.name, deps)
        })
        .collect())
}

fn is_tauri(dep: &str) -> bool {
    dep == "tauri" || dep.starts_with("tauri-")
}

fn is_leptos(dep: &str) -> bool {
    dep == "leptos" || dep.starts_with("leptos_") || dep.starts_with("leptos-")
}

fn is_internal(dep: &str) -> bool {
    dep.starts_with("ak-")
}

/// Returns one human-readable line per rule violation.
pub fn violations(graph: &DepGraph) -> Vec<String> {
    let mut out = Vec::new();
    for (krate, deps) in graph {
        let k = krate.as_str();
        for dep in deps {
            let d = dep.as_str();
            let mut deny = |why: &str| out.push(format!("{k} -> {d}: {why}"));
            if is_tauri(d) && !TAURI_ALLOWED.contains(&k) {
                deny("only ak-app and ak-bridge may depend on tauri");
            }
            if d == "anyhow" && !ANYHOW_ALLOWED.contains(&k) {
                deny("anyhow is only allowed in ak-app; use thiserror");
            }
            if CORE.contains(&k) && (is_leptos(d) || d == "ak-shell") {
                deny("core crates must not depend on leptos or ak-shell");
            }
            if k == "ak-types" && (is_internal(d) || is_leptos(d)) {
                deny("ak-types is the bottom layer");
            }
            if k == "ak-shell" && is_internal(d) && d != "ak-types" {
                deny("ak-shell may only depend on ak-types");
            }
            if GENERATED_APP.contains(&k) && is_internal(d) {
                deny("generated-app crates must not depend on other ak-* crates");
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(entries: &[(&str, &[&str])]) -> DepGraph {
        entries
            .iter()
            .map(|(k, ds)| (k.to_string(), ds.iter().map(|d| d.to_string()).collect()))
            .collect()
    }

    #[test]
    fn valid_graph_passes() {
        let g = graph(&[
            ("ak-types", &["serde"]),
            ("ak-agent", &["ak-types", "ak-providers", "thiserror"]),
            ("ak-shell", &["ak-types", "leptos"]),
            ("ak-app", &["ak-agent", "tauri", "tauri-build", "anyhow"]),
            ("ak-bridge", &["tauri", "leptos"]),
            ("ak-ui", &["leptos"]),
        ]);
        assert_eq!(violations(&g), Vec::<String>::new());
    }

    #[test]
    fn core_on_tauri_fails() {
        let g = graph(&[("ak-doctor", &["tauri"])]);
        assert_eq!(violations(&g).len(), 1);
    }

    #[test]
    fn core_on_leptos_or_shell_fails() {
        let g = graph(&[("ak-agent", &["leptos", "ak-shell"])]);
        assert_eq!(violations(&g).len(), 2);
    }

    #[test]
    fn shell_on_core_fails() {
        let g = graph(&[("ak-shell", &["ak-agent"])]);
        assert_eq!(violations(&g).len(), 1);
    }

    #[test]
    fn types_on_internal_fails() {
        let g = graph(&[("ak-types", &["ak-project"])]);
        assert_eq!(violations(&g).len(), 1);
    }

    #[test]
    fn generated_app_crate_on_internal_fails() {
        let g = graph(&[("ak-ui", &["ak-types"]), ("ak-bridge", &["ak-agent"])]);
        assert_eq!(violations(&g).len(), 2);
    }

    #[test]
    fn anyhow_outside_app_fails() {
        let g = graph(&[("ak-project", &["anyhow"])]);
        assert_eq!(violations(&g).len(), 1);
    }
}
