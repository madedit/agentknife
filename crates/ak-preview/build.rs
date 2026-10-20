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
    let out =
        PathBuf::from(std::env::var("OUT_DIR").unwrap_or_default()).join("tauri_leptos_files.rs");
    if let Err(e) = std::fs::write(&out, code) {
        panic!("writing {}: {e}", out.display());
    }
}
