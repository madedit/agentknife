//! `cargo xtask check-knowledge`: compiles the knowledge pack's code the way the
//! agent would use it — a project from the template with the complete example
//! (`example-todo.md`) as its code and the other topics' snippets
//! (`knowledge/<stack>/check/`) beside it, plus the 3D topic's `game.js` and
//! `game.rs` as a module, and the sound topic's code as `sound.rs` — using
//! AgentKnife's own toolchain.
//! Slow (a full build); run it after changing the docs or a pinned version.

use std::path::Path;
use std::process::Command;

use ak_doctor::{Pinned, Toolchain};

use crate::Result;

const STACK: &str = "tauri-leptos";

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let path = entry?.path();
        let name = path.file_name().unwrap_or_default();
        if name == "target" || name == "dist" || name == "template.toml" {
            continue;
        }
        if path.is_dir() {
            copy_dir(&path, &to.join(name))?;
        } else {
            std::fs::copy(&path, to.join(name))?;
        }
    }
    Ok(())
}

/// The ```rust blocks of a Markdown file, in order.
fn rust_blocks(markdown: &str) -> Vec<String> {
    blocks(markdown, "rust")
}

/// The fenced blocks of one language, in order. Only fences at the start of a
/// line count: indented ones (in a list) are fragments, not compiled.
fn blocks(markdown: &str, lang: &str) -> Vec<String> {
    format!("\n{markdown}")
        .split(&format!("\n```{lang}\n"))
        .skip(1)
        .filter_map(|rest| rest.split_once("```").map(|(code, _)| code.to_owned()))
        .collect()
}

fn add_deps(manifest: &Path, deps: &str) -> Result<()> {
    let text = std::fs::read_to_string(manifest)?;
    let text = text.replacen("[dependencies]\n", &format!("[dependencies]\n{deps}"), 1);
    std::fs::write(manifest, text)?;
    Ok(())
}

pub fn check_build(root: &Path) -> Result<()> {
    let out = root.join("target").join("knowledge-check");
    if out.exists() {
        std::fs::remove_dir_all(&out)?;
    }
    copy_dir(&root.join("templates").join(STACK), &out)?;
    std::fs::rename(out.join("gitignore"), out.join(".gitignore"))?;

    let pack = root.join("knowledge").join(STACK);
    let example = std::fs::read_to_string(pack.join("example-todo.md"))?;
    let [backend, screens] = <[String; 2]>::try_from(rust_blocks(&example))
        .map_err(|b| format!("example-todo.md: expected 2 rust blocks, found {}", b.len()))?;
    std::fs::write(
        out.join("src-tauri/src/main.rs"),
        format!("{backend}\n#[allow(dead_code)]\nmod snippets;\n"),
    )?;
    std::fs::write(
        out.join("src/main.rs"),
        format!(
            "{screens}\n#[allow(dead_code, unused)]\nmod snippets;\n#[allow(dead_code)]\nmod game;\n#[allow(dead_code)]\nmod sound;\n"
        ),
    )?;
    // The sound topic: its module and the screens that use it, as one file.
    let sound = std::fs::read_to_string(pack.join("sound.md"))?;
    let sound_rs = rust_blocks(&sound);
    if sound_rs.is_empty() {
        return Err("sound.md: no rust blocks (src/sound.rs)".into());
    }
    std::fs::write(out.join("src/sound.rs"), sound_rs.concat())?;
    // The 3D topic: its JS module (wasm-bindgen reads it while compiling) and screen.
    let three = std::fs::read_to_string(pack.join("three-d.md"))?;
    // The first js block is the whole game.js; later ones are fragments of it.
    let game_js = blocks(&three, "js")
        .into_iter()
        .next()
        .ok_or("three-d.md: no js block (src/game.js)")?;
    let [game_rs] = <[String; 1]>::try_from(rust_blocks(&three))
        .map_err(|b| format!("three-d.md: expected 1 rust block, found {}", b.len()))?;
    std::fs::write(out.join("src/game.js"), game_js)?;
    std::fs::write(out.join("src/game.rs"), game_rs)?;
    std::fs::copy(
        pack.join("check/backend.rs"),
        out.join("src-tauri/src/snippets.rs"),
    )?;
    std::fs::copy(pack.join("check/screens.rs"), out.join("src/snippets.rs"))?;
    // What the docs tell the agent to add.
    add_deps(
        &out.join("src-tauri/Cargo.toml"),
        "serde_json = \"1.0.154\"\nchrono = \"0.4.45\"\nimage = { version = \"0.25.10\", default-features = false, features = [\"jpeg\", \"png\", \"gif\", \"webp\"] }\nbase64 = \"0.22.1\"\ntauri-plugin-dialog = \"2.8.3\"\nrfd = \"0.16.1\"\nencoding_rs = \"0.8.42\"\ntauri-plugin-notification = \"2.6.0\"\ntauri-plugin-autostart = \"2.7.0\"\n",
    )?;
    // Topic `desktop`: the tray icon needs this tauri feature.
    let backend = out.join("src-tauri/Cargo.toml");
    let manifest = std::fs::read_to_string(&backend)?;
    let tray = manifest.replacen(
        "tauri = { version = \"=2.12.4\", features = [] }",
        "tauri = { version = \"=2.12.4\", features = [\"tray-icon\"] }",
        1,
    );
    if tray == manifest {
        return Err("template src-tauri/Cargo.toml: the tauri line was not found".into());
    }
    std::fs::write(&backend, tray)?;
    add_deps(
        &out.join("Cargo.toml"),
        "serde_json = \"1.0.154\"\njs-sys = \"0.3.106\"\nwasm-bindgen-futures = \"0.4.79\"\nweb-sys = { version = \"0.3.106\", features = [\"HtmlCanvasElement\", \"Storage\", \"Window\", \"Navigator\", \"Clipboard\", \"Document\", \"Element\", \"HtmlInputElement\", \"EventTarget\", \"Request\", \"RequestInit\", \"Response\", \"AbortController\", \"AbortSignal\", \"HtmlDocument\", \"Node\", \"Text\", \"CharacterData\", \"DragEvent\", \"DataTransfer\", \"DomRect\", \"AddEventListenerOptions\", \"WheelEvent\", \"PointerEvent\", \"HtmlElement\", \"ClipboardEvent\", \"CanvasRenderingContext2d\", \"AudioBuffer\", \"AudioBufferSourceNode\", \"AudioContext\", \"AudioDestinationNode\", \"AudioNode\", \"AudioParam\", \"AudioScheduledSourceNode\", \"GainNode\", \"HtmlAudioElement\", \"HtmlMediaElement\", \"MediaElementAudioSourceNode\", \"AnalyserNode\", \"DomTokenList\", \"MediaQueryList\"] }\n",
    )?;

    let pinned = Pinned::builtin()?;
    let tc = Toolchain::new(crate::toolchain_root(), &pinned);
    let cargo = tc.exe(&tc.cargo_bin(), "cargo");
    if !cargo.exists() {
        return Err(format!(
            "{} is missing: install AgentKnife's toolchain first (doctor_install example)",
            cargo.display()
        )
        .into());
    }
    let mut path = tc.path_prefix();
    path.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let builds: [&[&str]; 2] = [
        &[
            "build",
            "--target",
            "wasm32-unknown-unknown",
            "--features",
            "dev",
        ],
        &[
            "build",
            "--manifest-path",
            "src-tauri/Cargo.toml",
            "--features",
            "dev",
        ],
    ];
    for args in builds {
        println!("==> cargo {}", args.join(" "));
        let status = Command::new(&cargo)
            .args(args)
            .current_dir(&out)
            .envs(tc.env())
            .env("PATH", std::env::join_paths(&path)?)
            .env_remove("NO_COLOR")
            .env_remove("CARGO")
            .env_remove("RUSTC")
            .status()?;
        if !status.success() {
            return Err(format!("knowledge code does not build: cargo {}", args.join(" ")).into());
        }
    }
    println!("knowledge code builds (project in {})", out.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_rust_blocks_only() {
        let md = "# T\n```rust\nfn a() {}\n```\ntext\n```text\nno\n```\n```rust\nfn b() {}\n```\n";
        assert_eq!(rust_blocks(md), ["fn a() {}\n", "fn b() {}\n"]);
        let listed = "- a list item:\n  ```rust\n  let x = 1;\n  ```\n";
        assert!(
            rust_blocks(listed).is_empty(),
            "indented fragments are skipped"
        );
    }
}
