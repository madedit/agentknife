//! `cargo xtask vendor-kit`: copies the crates generated apps use (ak-bridge,
//! ak-ui) into the template's `ak/` folder as standalone packages, and the CI
//! check that those copies are current.
//!
//! Generated projects live outside this repo and these crates are not
//! published, so each project carries its own copy (refreshed by AgentKnife).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use toml::{Table, Value};

use crate::Result;

/// The crates generated apps depend on.
const KIT: &[&str] = &["ak-bridge", "ak-ui"];
/// Template folders that carry a copy (relative to the repo root).
pub const TEMPLATES: &[&str] = &["templates/tauri-leptos"];

/// Every file of the copies, by path relative to the template folder.
fn files(root: &Path) -> Result<BTreeMap<PathBuf, Vec<u8>>> {
    let ws: Table = toml::from_str(&std::fs::read_to_string(root.join("Cargo.toml"))?)?;
    let ws_deps = ws
        .get("workspace")
        .and_then(|w| w.get("dependencies"))
        .and_then(Value::as_table)
        .ok_or("Cargo.toml has no [workspace.dependencies]")?;
    let ws_pkg = ws
        .get("workspace")
        .and_then(|w| w.get("package"))
        .and_then(Value::as_table)
        .ok_or("Cargo.toml has no [workspace.package]")?;
    let mut out = BTreeMap::new();
    for krate in KIT {
        let dir = root.join("crates").join(krate);
        let manifest: Table = toml::from_str(&std::fs::read_to_string(dir.join("Cargo.toml"))?)?;
        let text = standalone_manifest(krate, &manifest, ws_deps, ws_pkg)?;
        let base = PathBuf::from("ak").join(krate);
        out.insert(base.join("Cargo.toml"), text.into_bytes());
        for file in walk(&dir.join("src"))? {
            let rel = file.strip_prefix(&dir)?;
            // Normalise line endings so Windows checkouts compare equal.
            let bytes = String::from_utf8(std::fs::read(&file)?)?
                .replace("\r\n", "\n")
                .into_bytes();
            out.insert(base.join(rel), bytes);
        }
    }
    Ok(out)
}

fn walk(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            out.extend(walk(&path)?);
        } else {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

/// One dependency with `workspace = true` replaced by the workspace's spec.
fn resolve(name: &str, spec: &Value, ws_deps: &Table) -> Result<Value> {
    let Some(t) = spec.as_table() else {
        return Ok(spec.clone());
    };
    if t.get("workspace").and_then(Value::as_bool) != Some(true) {
        return Ok(spec.clone());
    }
    let mut out = match ws_deps.get(name) {
        Some(Value::String(v)) => {
            let mut t = Table::new();
            t.insert("version".into(), Value::String(v.clone()));
            t
        }
        Some(Value::Table(t)) => t.clone(),
        _ => return Err(format!("{name} is not in [workspace.dependencies]").into()),
    };
    if out.contains_key("path") {
        return Err(format!("{name}: the kit must not depend on workspace path crates").into());
    }
    for (k, v) in t {
        match k.as_str() {
            "workspace" => {}
            "features" => {
                let mut all: Vec<Value> = out
                    .get("features")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                for f in v.as_array().into_iter().flatten() {
                    if !all.contains(f) {
                        all.push(f.clone());
                    }
                }
                out.insert("features".into(), Value::Array(all));
            }
            _ => {
                out.insert(k.clone(), v.clone());
            }
        }
    }
    Ok(Value::Table(out))
}

fn resolve_all(deps: Option<&Value>, ws_deps: &Table) -> Result<Option<Table>> {
    let Some(deps) = deps.and_then(Value::as_table) else {
        return Ok(None);
    };
    let mut out = Table::new();
    for (name, spec) in deps {
        out.insert(name.clone(), resolve(name, spec, ws_deps)?);
    }
    Ok(Some(out))
}

fn section(name: &str, value: Table) -> Result<String> {
    let mut t = Table::new();
    t.insert(name.to_owned(), Value::Table(value));
    Ok(toml::to_string(&t)?)
}

fn standalone_manifest(
    krate: &str,
    manifest: &Table,
    ws_deps: &Table,
    ws_pkg: &Table,
) -> Result<String> {
    let pkg = manifest
        .get("package")
        .and_then(Value::as_table)
        .ok_or_else(|| format!("{krate}: no [package]"))?;
    let mut package = Table::new();
    for (k, v) in pkg {
        let inherited = v
            .as_table()
            .and_then(|t| t.get("workspace"))
            .and_then(Value::as_bool)
            == Some(true);
        let value = if inherited {
            ws_pkg
                .get(k)
                .cloned()
                .ok_or_else(|| format!("{krate}: workspace.package.{k} missing"))?
        } else {
            v.clone()
        };
        package.insert(k.clone(), value);
    }
    package.insert("publish".into(), Value::Boolean(false));
    // Only the sources are copied; the repo's build script is for its tests.
    package.insert("build".into(), Value::Boolean(false));

    let mut text = format!(
        "# Copied from crates/{krate} by `cargo xtask vendor-kit`. Do not edit here:\n\
         # change the original and re-run it. AgentKnife refreshes this folder.\n\n"
    );
    text.push_str(&section("package", package)?);
    if let Some(features) = manifest.get("features").and_then(Value::as_table) {
        text.push('\n');
        text.push_str(&section("features", features.clone())?);
    }
    if let Some(deps) = resolve_all(manifest.get("dependencies"), ws_deps)? {
        text.push('\n');
        text.push_str(&section("dependencies", deps)?);
    }
    if let Some(targets) = manifest.get("target").and_then(Value::as_table) {
        let mut out = Table::new();
        for (cfg, t) in targets {
            if let Some(deps) = resolve_all(t.get("dependencies"), ws_deps)? {
                let mut inner = Table::new();
                inner.insert("dependencies".into(), Value::Table(deps));
                out.insert(cfg.clone(), Value::Table(inner));
            }
        }
        if !out.is_empty() {
            text.push('\n');
            text.push_str(&section("target", out)?);
        }
    }
    Ok(text)
}

/// Rewrites `ak/` in every template.
pub fn vendor(root: &Path) -> Result<()> {
    let files = files(root)?;
    for template in TEMPLATES {
        let base = root.join(template);
        for krate in KIT {
            let dir = base.join("ak").join(krate);
            if dir.exists() {
                std::fs::remove_dir_all(&dir)?;
            }
        }
        for (rel, bytes) in &files {
            let path = base.join(rel);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, bytes)?;
        }
        println!("vendored {} files into {template}/ak", files.len());
    }
    Ok(())
}

/// CI: the copies equal what `vendor` would write (no stale, missing or extra files).
pub fn check(root: &Path) -> Result<()> {
    let files = files(root)?;
    let mut errors = Vec::new();
    for template in TEMPLATES {
        let base = root.join(template);
        for (rel, bytes) in &files {
            let on_disk = std::fs::read(base.join(rel)).ok().map(|b| {
                String::from_utf8_lossy(&b)
                    .replace("\r\n", "\n")
                    .into_bytes()
            });
            if on_disk.as_ref() != Some(bytes) {
                errors.push(format!("{template}/{} is out of date", rel.display()));
            }
        }
        for krate in KIT {
            let dir = base.join("ak").join(krate);
            if !dir.is_dir() {
                continue;
            }
            for file in walk(&dir)? {
                let rel = file.strip_prefix(&base)?.to_path_buf();
                if !files.contains_key(&rel) {
                    errors.push(format!("{template}/{} is not in the kit", rel.display()));
                }
            }
        }
    }
    if !errors.is_empty() {
        errors.push("run `cargo xtask vendor-kit`".into());
        return Err(errors.join("\n").into());
    }
    // Each template's lockfile must match its manifests (new projects build
    // with exactly these versions).
    for template in TEMPLATES {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
        let out = std::process::Command::new(cargo)
            .args(["tree", "--locked", "-q", "--depth", "0", "--manifest-path"])
            .arg(root.join(template).join("Cargo.toml"))
            .output()?;
        if !out.status.success() {
            return Err(format!(
                "{template}/Cargo.lock does not match its manifests (update it with \
                 `cargo update -w --manifest-path {template}/Cargo.toml`):\n{}",
                String::from_utf8_lossy(&out.stderr)
            )
            .into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_specs_are_inlined_with_member_features() {
        let ws: Table = toml::from_str(
            r#"
            serde = { version = "1.0.229", features = ["derive"] }
            tokio = "1.54.0"
            "#,
        )
        .unwrap();
        let member: Value = toml::from_str::<Table>(
            r#"x = { workspace = true, optional = true, features = ["net", "derive"] }"#,
        )
        .unwrap()["x"]
            .clone();
        let r = resolve("serde", &member, &ws).unwrap();
        assert_eq!(r["version"].as_str(), Some("1.0.229"));
        assert_eq!(r["optional"].as_bool(), Some(true));
        let features: Vec<&str> = r["features"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert_eq!(features, ["derive", "net"]);
        assert!(r.get("workspace").is_none());
        let plain = resolve(
            "tokio",
            &Value::Table(member.as_table().unwrap().clone()),
            &ws,
        )
        .unwrap();
        assert_eq!(plain["version"].as_str(), Some("1.54.0"));
        assert!(resolve("missing", &member, &ws).is_err());
    }
}
