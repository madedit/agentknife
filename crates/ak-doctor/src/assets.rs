//! Where each pinned tool is downloaded from, per target (asset names checked
//! against the releases on 2026-10-20). Checksums live in `versions.toml`
//! `[downloads]`, filled by `cargo xtask pin-downloads`.

use crate::toolchain::Pinned;

/// Targets with prebuilt downloads; anything else falls back to `cargo install`.
pub const PREBUILT_TARGETS: &[&str] = &[
    "x86_64-pc-windows-msvc",
    "x86_64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Archive {
    /// The download is the program itself.
    Raw,
    Zip,
    TarGz,
}

/// Which toolchain folder the program goes into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dest {
    CargoBin,
    Bin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    /// Doctor item id (`trunk`, `tailwind`, …) or `rustup-init`.
    pub tool: &'static str,
    /// `[downloads]` key: `<tool>@<version>/<target>`.
    pub key: String,
    pub url: String,
    pub file_name: String,
    pub archive: Archive,
    /// The program's file name inside the archive (and once installed).
    pub binary: String,
    pub dest: Dest,
    /// The publisher's checksum file, when there is one.
    pub checksum_url: Option<String>,
}

/// The target this AgentKnife build runs on, if it has prebuilt downloads.
pub fn host_target() -> Option<&'static str> {
    let t = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        _ => return None,
    };
    Some(t)
}

fn exe(target: &str, name: &str) -> String {
    if target.contains("windows") {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

/// The official installer for the pinned rustup (any Rust target triple).
pub fn rustup_init(pinned: &Pinned, target: &str) -> Asset {
    let v = &pinned.rustup;
    let file_name = exe(target, "rustup-init");
    let url = format!("https://static.rust-lang.org/rustup/archive/{v}/{target}/{file_name}");
    Asset {
        tool: "rustup-init",
        key: format!("rustup-init@{v}/{target}"),
        checksum_url: Some(format!("{url}.sha256")),
        url,
        binary: file_name.clone(),
        file_name,
        archive: Archive::Raw,
        dest: Dest::Bin,
    }
}

/// The prebuilt download of `tool` for `target`, if one exists.
pub fn prebuilt(tool: &str, pinned: &Pinned, target: &str) -> Option<Asset> {
    if !PREBUILT_TARGETS.contains(&target) {
        return None;
    }
    let windows = target.contains("windows");
    let mac = target.contains("apple");
    let asset = match tool {
        "trunk" => {
            let v = &pinned.trunk;
            let file_name = format!("trunk-{target}.{}", if windows { "zip" } else { "tar.gz" });
            let url =
                format!("https://github.com/trunk-rs/trunk/releases/download/v{v}/{file_name}");
            Asset {
                tool: "trunk",
                key: format!("trunk@{v}/{target}"),
                checksum_url: Some(format!("{url}.sha256")),
                url,
                file_name,
                archive: if windows {
                    Archive::Zip
                } else {
                    Archive::TarGz
                },
                binary: exe(target, "trunk"),
                dest: Dest::CargoBin,
            }
        }
        "wasm-bindgen" => {
            let v = &pinned.wasm_bindgen;
            // Linux x64 is only published as a static (musl) build.
            let triple = if target == "x86_64-unknown-linux-gnu" {
                "x86_64-unknown-linux-musl"
            } else {
                target
            };
            let file_name = format!("wasm-bindgen-{v}-{triple}.tar.gz");
            let url = format!(
                "https://github.com/wasm-bindgen/wasm-bindgen/releases/download/{v}/{file_name}"
            );
            Asset {
                tool: "wasm-bindgen",
                key: format!("wasm-bindgen@{v}/{target}"),
                checksum_url: Some(format!("{url}.sha256sum")),
                url,
                file_name,
                archive: Archive::TarGz,
                binary: exe(target, "wasm-bindgen"),
                dest: Dest::CargoBin,
            }
        }
        "tauri-cli" => {
            let v = &pinned.tauri_cli;
            let ext = if windows || mac { "zip" } else { "tgz" };
            let file_name = format!("cargo-tauri-{target}.{ext}");
            Asset {
                tool: "tauri-cli",
                key: format!("tauri-cli@{v}/{target}"),
                url: format!(
                    "https://github.com/tauri-apps/tauri/releases/download/tauri-cli-v{v}/{file_name}"
                ),
                file_name,
                archive: if ext == "zip" {
                    Archive::Zip
                } else {
                    Archive::TarGz
                },
                binary: exe(target, "cargo-tauri"),
                dest: Dest::CargoBin,
                // tauri does not publish checksums: xtask hashes the download.
                checksum_url: None,
            }
        }
        "tailwind" => {
            let v = &pinned.tailwind;
            let suffix = match target {
                "x86_64-pc-windows-msvc" => "windows-x64.exe",
                "x86_64-unknown-linux-gnu" => "linux-x64",
                "x86_64-apple-darwin" => "macos-x64",
                _ => "macos-arm64",
            };
            let file_name = format!("tailwindcss-{suffix}");
            let base =
                format!("https://github.com/tailwindlabs/tailwindcss/releases/download/v{v}");
            Asset {
                tool: "tailwind",
                key: format!("tailwind@{v}/{target}"),
                url: format!("{base}/{file_name}"),
                checksum_url: Some(format!("{base}/sha256sums.txt")),
                binary: exe(target, "tailwindcss"),
                file_name,
                archive: Archive::Raw,
                dest: Dest::Bin,
            }
        }
        _ => return None,
    };
    Some(asset)
}

/// Every pinned download for every prebuilt target (for `xtask pin-downloads`).
pub fn all_assets(pinned: &Pinned) -> Vec<Asset> {
    let mut out = Vec::new();
    for target in PREBUILT_TARGETS {
        out.push(rustup_init(pinned, target));
        for tool in ["trunk", "wasm-bindgen", "tauri-cli", "tailwind"] {
            out.extend(prebuilt(tool, pinned, target));
        }
    }
    out
}

/// Reads a checksum file: a bare hash, `hash  name`, or many `hash  ./name` lines.
pub fn parse_checksum(text: &str, file_name: &str) -> Option<String> {
    let is_hash = |s: &str| s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit());
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let pick = |l: &&str| {
        let mut parts = l.split_whitespace();
        let hash = parts.next()?;
        // `name`, `*name`, `./name`, or a path such as `gh-release/name`.
        let name = parts
            .next()
            .map(|n| n.trim_start_matches('*').rsplit('/').next().unwrap_or(n));
        (is_hash(hash) && name.is_none_or(|n| n == file_name)).then(|| hash.to_ascii_lowercase())
    };
    lines.iter().find_map(pick)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pinned() -> Pinned {
        Pinned {
            rust: "1.99.1".into(),
            rustup: "1.29.1".into(),
            trunk: "0.21.14".into(),
            wasm_bindgen: "0.2.129".into(),
            tauri_cli: "2.12.3".into(),
            tailwind: "4.3.3".into(),
            downloads: Default::default(),
        }
    }

    #[test]
    fn urls_match_the_published_asset_names() {
        let p = pinned();
        let win = "x86_64-pc-windows-msvc";
        assert_eq!(
            rustup_init(&p, win).url,
            "https://static.rust-lang.org/rustup/archive/1.29.1/x86_64-pc-windows-msvc/rustup-init.exe"
        );
        assert_eq!(
            prebuilt("trunk", &p, win).unwrap().url,
            "https://github.com/trunk-rs/trunk/releases/download/v0.21.14/trunk-x86_64-pc-windows-msvc.zip"
        );
        let wb = prebuilt("wasm-bindgen", &p, "x86_64-unknown-linux-gnu").unwrap();
        assert!(
            wb.file_name.ends_with("x86_64-unknown-linux-musl.tar.gz"),
            "musl on Linux x64"
        );
        let tauri_mac = prebuilt("tauri-cli", &p, "aarch64-apple-darwin").unwrap();
        assert_eq!(tauri_mac.file_name, "cargo-tauri-aarch64-apple-darwin.zip");
        assert_eq!(tauri_mac.archive, Archive::Zip);
        assert_eq!(
            prebuilt("tauri-cli", &p, "x86_64-unknown-linux-gnu")
                .unwrap()
                .archive,
            Archive::TarGz
        );
        let tw = prebuilt("tailwind", &p, win).unwrap();
        assert_eq!(tw.file_name, "tailwindcss-windows-x64.exe");
        assert_eq!(tw.binary, "tailwindcss.exe");
        assert!(
            prebuilt("trunk", &p, "riscv64gc-unknown-linux-gnu").is_none(),
            "falls back"
        );
        assert_eq!(all_assets(&p).len(), 4 * 5);
    }

    #[test]
    fn checksum_formats() {
        let h = "6f4bef66261261fcb43131be8720bab817d403a09edec7455c371974b90bdb7e";
        assert_eq!(parse_checksum(&format!("{h}\n"), "x").as_deref(), Some(h));
        assert_eq!(
            parse_checksum(&format!("{h}  trunk.zip\n"), "trunk.zip").as_deref(),
            Some(h)
        );
        assert_eq!(
            parse_checksum(&format!("{h} *rustup-init.exe"), "rustup-init.exe").as_deref(),
            Some(h)
        );
        let sums = format!(
            "{}  ./tailwindcss-linux-x64\n{h}  ./tailwindcss-windows-x64.exe\n",
            "a".repeat(64)
        );
        assert_eq!(
            parse_checksum(&sums, "tailwindcss-windows-x64.exe").as_deref(),
            Some(h)
        );
        assert_eq!(parse_checksum("not a hash", "x"), None);
        // wasm-bindgen's format: the name carries a path.
        assert_eq!(
            parse_checksum(
                &format!("{h}  gh-release/wb-0.2.129.tar.gz\n"),
                "wb-0.2.129.tar.gz"
            )
            .as_deref(),
            Some(h)
        );
    }
}
