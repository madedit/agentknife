//! `cargo xtask dist`: the release zip of AgentKnife for this computer,
//! `target/dist/AgentKnife-<version>-<os>-<arch>.zip`. It runs `cargo xtask
//! build` first, then packs both programs (agentknife-mcp must stay next to
//! AgentKnife), the licenses, the third-party notices and a short README.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use ak_doctor::{Pinned, Toolchain};
use serde_json::Value;

use crate::Result;

/// Programs in the zip: AgentKnife's backend and its MCP program.
const NATIVE_ROOTS: &[&str] = &["ak-app", "ak-mcp"];
/// The interface, compiled to wasm32 and embedded in AgentKnife.
const WASM_ROOTS: &[&str] = &["ak-shell"];

pub fn run(root: &Path) -> Result<()> {
    crate::dev::build(root, &[])?;
    let version = version(root)?;
    let target = ak_doctor::assets::host_target().ok_or("no release target for this computer")?;
    let name = format!("AgentKnife-{version}-{}", platform());
    let exe = std::env::consts::EXE_SUFFIX;
    let release = root.join("target/release");

    let mut files: Vec<(String, Vec<u8>, bool)> = Vec::new();
    for program in ["agentknife", "agentknife-mcp"] {
        let file = format!("{program}{exe}");
        files.push((file.clone(), std::fs::read(release.join(&file))?, true));
    }
    for license in ["LICENSE-MIT", "LICENSE-APACHE"] {
        files.push((license.into(), std::fs::read(root.join(license))?, false));
    }
    let mut notices = third_party(root, target, NATIVE_ROOTS)?;
    notices.extend(third_party(root, "wasm32-unknown-unknown", WASM_ROOTS)?);
    files.push((
        "THIRD-PARTY-NOTICES.txt".into(),
        notices_text(&notices).into_bytes(),
        false,
    ));
    files.push((
        "README.txt".into(),
        readme(&version, exe).into_bytes(),
        false,
    ));

    let out_dir = root.join("target/dist");
    std::fs::create_dir_all(&out_dir)?;
    let out = out_dir.join(format!("{name}.zip"));
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&out)?);
    for (file, data, program) in &files {
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(if *program { 0o755 } else { 0o644 });
        zip.start_file(format!("{name}/{file}"), options)?;
        zip.write_all(data)?;
    }
    zip.finish()?;
    let size = std::fs::metadata(&out)?.len() as f64 / 1_048_576.0;
    println!(
        "wrote {} ({size:.1} MB, {} packages in the notices)",
        out.display(),
        notices.len()
    );
    Ok(())
}

/// `[workspace.package] version`, which AgentKnife reports as its own.
fn version(root: &Path) -> Result<String> {
    let ws: toml::Table = toml::from_str(&std::fs::read_to_string(root.join("Cargo.toml"))?)?;
    ws.get("workspace")
        .and_then(|w| w.get("package"))
        .and_then(|p| p.get("version"))
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| "Cargo.toml has no workspace.package.version".into())
}

/// "windows-x64", "macos-arm64", "linux-x64".
fn platform() -> String {
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    };
    format!("{}-{arch}", std::env::consts::OS)
}

/// One third-party package and the license files it ships.
struct Package {
    name: String,
    version: String,
    license: String,
    texts: Vec<String>,
}

/// Every non-workspace package that `roots` use as a normal dependency on `target`.
fn third_party(root: &Path, target: &str, roots: &[&str]) -> Result<Vec<Package>> {
    let pinned = Pinned::builtin()?;
    let tc = Toolchain::new(Toolchain::default_root(), &pinned);
    let output = Command::new(tc.exe(&tc.cargo_bin(), "cargo"))
        .args([
            "metadata",
            "--format-version",
            "1",
            "--filter-platform",
            target,
        ])
        .current_dir(root)
        .envs(tc.env())
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let meta: Value = serde_json::from_slice(&output.stdout)?;
    let packages: BTreeMap<&str, &Value> = meta["packages"]
        .as_array()
        .ok_or("cargo metadata: no packages")?
        .iter()
        .filter_map(|p| Some((p["id"].as_str()?, p)))
        .collect();
    let nodes: BTreeMap<&str, &Value> = meta["resolve"]["nodes"]
        .as_array()
        .ok_or("cargo metadata: no resolve")?
        .iter()
        .filter_map(|n| Some((n["id"].as_str()?, n)))
        .collect();

    let mut queue: VecDeque<&str> = packages
        .iter()
        .filter(|(_, p)| roots.contains(&p["name"].as_str().unwrap_or_default()))
        .map(|(id, _)| *id)
        .collect();
    let mut seen: BTreeSet<&str> = queue.iter().copied().collect();
    while let Some(id) = queue.pop_front() {
        let Some(deps) = nodes.get(id).and_then(|n| n["deps"].as_array()) else {
            continue;
        };
        for dep in deps {
            let normal = dep["dep_kinds"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|k| k["kind"].is_null()));
            if let Some(pkg) = dep["pkg"].as_str()
                && normal
                && seen.insert(pkg)
            {
                queue.push_back(pkg);
            }
        }
    }

    let mut out = Vec::new();
    for id in seen {
        let Some(p) = packages.get(id) else { continue };
        // Path packages are AgentKnife's own.
        if p["source"].is_null() {
            continue;
        }
        let dir = Path::new(p["manifest_path"].as_str().unwrap_or_default())
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        out.push(Package {
            name: p["name"].as_str().unwrap_or_default().into(),
            version: p["version"].as_str().unwrap_or_default().into(),
            license: p["license"].as_str().unwrap_or("see the package").into(),
            texts: license_texts(&dir, p["license_file"].as_str()),
        });
    }
    Ok(out)
}

/// LICENSE*, LICENCE*, COPYING*, NOTICE* and the manifest's license-file.
fn license_texts(dir: &Path, license_file: Option<&str>) -> Vec<String> {
    let mut paths: BTreeSet<PathBuf> = BTreeSet::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_uppercase();
            let wanted = ["LICENSE", "LICENCE", "COPYING", "NOTICE"]
                .iter()
                .any(|p| name.starts_with(p));
            if wanted && entry.path().is_file() {
                paths.insert(entry.path());
            }
        }
    }
    if let Some(file) = license_file {
        paths.insert(dir.join(file));
    }
    paths
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .map(|t| t.replace("\r\n", "\n").trim().to_owned())
        .filter(|t| !t.is_empty())
        .collect()
}

/// Packages grouped by identical license texts, then the bundled JavaScript.
fn notices_text(packages: &[Package]) -> String {
    let mut groups: BTreeMap<Vec<String>, BTreeSet<String>> = BTreeMap::new();
    for p in packages {
        groups
            .entry(p.texts.clone())
            .or_default()
            .insert(format!("{} {} ({})", p.name, p.version, p.license));
    }
    let mut out = String::from(
        "AgentKnife includes the third-party software listed below, under the\n\
         licenses that follow each list.\n",
    );
    let rule = "=".repeat(78);
    for (texts, names) in &groups {
        out.push_str(&format!("\n{rule}\n"));
        for n in names {
            out.push_str(&format!("{n}\n"));
        }
        out.push_str(&format!("{rule}\n\n"));
        if texts.is_empty() {
            out.push_str("No license file in the package; see its license above.\n");
        }
        for t in texts {
            out.push_str(t);
            out.push_str("\n\n");
        }
    }
    out.push_str(&format!("\n{rule}\n{JS_NOTICES}"));
    out
}

/// The JavaScript bundled into AgentKnife's interface (crates/ak-shell/vendor)
/// and the Tailwind CSS base styles in its stylesheet.
const JS_NOTICES: &str = "\
CodeMirror 6 and Lezer (codemirror, @codemirror/*, @lezer/*) with their bundled
dependencies style-mod, crelt, w3c-keyname and @marijn/find-cluster-break
Copyright (C) 2018-2021 by Marijn Haverbeke <marijn@haverbeke.berlin> and others

unenv (Node.js module shims added by esm.sh)
Copyright (c) Pooya Parsa <pooya@pi0.io>

Tailwind CSS (base styles)
Copyright (c) Tailwind Labs, Inc.
==============================================================================

These are under the MIT License:

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the \"Software\"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
";

fn readme(version: &str, exe: &str) -> String {
    format!(
        "AgentKnife {version}\n\
         \n\
         Start agentknife{exe}. Nothing needs to be installed first: on first launch\n\
         AgentKnife connects to an AI, then installs the tools it builds apps with\n\
         (about 270 MB to download), and asks before anything is installed\n\
         system-wide (the Visual C++ Build Tools, about 1.5-2 GB).\n\
         \n\
         Keep agentknife-mcp{exe} in the same folder as agentknife{exe}: AI tools such\n\
         as Claude Code, Codex or Antigravity use it to operate AgentKnife.\n\
         \n\
         The programs are not signed. If Windows shows \"Windows protected your PC\",\n\
         choose \"More info\", then \"Run anyway\".\n\
         \n\
         AgentKnife is licensed under either of the Apache License 2.0\n\
         (LICENSE-APACHE) or the MIT license (LICENSE-MIT), at your option.\n\
         Third-party software and its licenses: THIRD-PARTY-NOTICES.txt.\n"
    )
}
