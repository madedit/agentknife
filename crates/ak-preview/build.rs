//! Embeds `templates/tauri-leptos` (the files a new project gets) as a static
//! list of `TemplateFile`s.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Not part of a project: build output and the template's own metadata.
const SKIP: &[&str] = &[
    "target",
    "dist",
    "template.toml",
    "src-tauri/gen",
    "src-tauri/target",
];

fn walk(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        if SKIP.contains(&rel.as_str()) {
            continue;
        }
        if path.is_dir() {
            walk(root, &path, out);
        } else {
            out.push(path);
        }
    }
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let root = manifest.join("../../templates/tauri-leptos");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut files = Vec::new();
    walk(&root, &root, &mut files);
    files.sort();
    let mut code = String::from("&[\n");
    for file in files {
        let rel = file
            .strip_prefix(&root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let abs = file.canonicalize().unwrap_or(file);
        let _ = writeln!(
            code,
            "    ak_types::stack::TemplateFile {{ path: {rel:?}, contents: include_bytes!({:?}) }},",
            abs.to_string_lossy()
        );
    }
    code.push(']');
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap_or_default());
    let out = out_dir.join("tauri_leptos_files.rs");
    if let Err(e) = std::fs::write(&out, code) {
        panic!("writing {}: {e}", out.display());
    }
    knowledge(&manifest.join("../../knowledge/tauri-leptos"), &out_dir);

    // Tests link Tauri (ak-bridge's server); on Windows Tauri needs Common-Controls
    // v6, or the test binaries fail to start (see crates/ak-bridge/build.rs).
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-tests=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );
    }
}

/// `knowledge/tauri-leptos`: pack.toml's id, version and topics, with each
/// topic's Markdown embedded.
fn knowledge(dir: &Path, out_dir: &Path) {
    println!("cargo:rerun-if-changed={}", dir.display());
    let text = std::fs::read_to_string(dir.join("pack.toml"))
        .unwrap_or_else(|e| panic!("{}/pack.toml: {e}", dir.display()));
    let pack: toml::Table =
        toml::from_str(&text).unwrap_or_else(|e| panic!("{}/pack.toml: {e}", dir.display()));
    let field = |t: &toml::Value, key: &str| -> String {
        t.get(key)
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("pack.toml: missing {key}"))
            .to_owned()
    };
    let meta = pack
        .get("pack")
        .unwrap_or_else(|| panic!("pack.toml: missing [pack]"));
    let mut code = format!(
        "pub(crate) const KNOWLEDGE_ID: &str = {:?};\npub(crate) const KNOWLEDGE_VERSION: &str = {:?};\n\
         pub(crate) static KNOWLEDGE_TOPICS: &[ak_types::stack::KnowledgeTopic] = &[\n",
        field(meta, "id"),
        field(meta, "version")
    );
    for topic in pack
        .get("topic")
        .and_then(|t| t.as_array())
        .into_iter()
        .flatten()
    {
        let file = dir.join(field(topic, "file"));
        let abs = file
            .canonicalize()
            .unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        let _ = writeln!(
            code,
            "    ak_types::stack::KnowledgeTopic {{ id: {:?}, title: {:?}, summary: {:?}, body: include_str!({:?}) }},",
            field(topic, "id"),
            field(topic, "title"),
            field(topic, "summary"),
            abs.to_string_lossy()
        );
    }
    code.push_str("];\n");
    code.push_str("pub(crate) static KNOWLEDGE_LIBRARIES: &[ak_types::stack::Library] = &[\n");
    for lib in pack
        .get("library")
        .and_then(|t| t.as_array())
        .into_iter()
        .flatten()
    {
        let lib_dir = dir.join(field(lib, "dir"));
        let mut files = Vec::new();
        walk(&lib_dir, &lib_dir, &mut files);
        files.sort();
        let _ = writeln!(
            code,
            "    ak_types::stack::Library {{ id: {:?}, version: {:?}, summary: {:?}, target: {:?}, files: &[",
            field(lib, "id"),
            field(lib, "version"),
            field(lib, "summary"),
            field(lib, "target"),
        );
        for file in files {
            let rel = file
                .strip_prefix(&lib_dir)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            // The vendoring checksums stay in AgentKnife's repo.
            if rel == "SHA256SUMS" {
                continue;
            }
            let abs = file.canonicalize().unwrap_or(file);
            let _ = writeln!(
                code,
                "        ak_types::stack::TemplateFile {{ path: {rel:?}, contents: include_bytes!({:?}) }},",
                abs.to_string_lossy()
            );
        }
        code.push_str("    ] },\n");
    }
    code.push_str("];\n");
    let out = out_dir.join("tauri_leptos_knowledge.rs");
    if let Err(e) = std::fs::write(&out, code) {
        panic!("writing {}: {e}", out.display());
    }
}
