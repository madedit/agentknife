# What gets downloaded

What a Windows 11 PC needs when Rust, its toolchain and the Visual C++ Build Tools are not installed yet. AgentKnife checks for each item and either installs it or guides you through it. Numbers marked ~ are estimates; the others were measured from the official downloads and from an installed copy.

## AgentKnife's own tools

They are installed in `%APPDATA%\AgentKnife\toolchain`, without changing your PATH.

| Package | Download | Installed |
|---|---:|---:|
| rustup (`rustup-init.exe`) | 12.1 MB | 12.1 MB |
| Rust: rustc, cargo, rust-std (minimal profile) | 105.4 MB | 690 MB (with the wasm32 target) |
| Rust wasm32 target (`rust-std-wasm32-unknown-unknown`) | 22.1 MB | (included above) |
| Trunk | 7.0 MB | 26.8 MB |
| wasm-bindgen CLI | 7.5 MB | 5.9 MB |
| tauri-cli (`cargo-tauri`) | 7.4 MB | 15.1 MB |
| Tailwind CSS CLI | 107.3 MB | 107.3 MB |
| **Total** | **268.8 MB** | **857 MB** |

## System-wide installs

AgentKnife asks before installing these, through winget.

| Package | Download | Installed | Notes |
|---|---:|---:|---|
| Visual Studio Build Tools, "Desktop development with C++" workload (MSVC, Windows SDK) | ~1.5–2 GB | ~6–7 GB | The installer itself is a few MB and downloads the rest. Most of it goes on drive C:. |
| Microsoft Edge WebView2 Runtime | ~2 MB (bootstrapper) + ~150 MB | ~500 MB | Already part of Windows 11, so usually nothing to install. |

## Per project

| Item | Download | Disk | Notes |
|---|---:|---:|---|
| Rust libraries the app uses (crates) | ~110 MB | ~1 GB | Downloaded on the first build into `toolchain\cargo\registry`, then shared by all projects. |
| Build output for the live preview (`<project>\target`) | 0 | ~2.5 GB | Measured on a sample spreadsheet app. Packaging adds a release build on top. |

## In total

The download is about 2–2.5 GB. The disk space needed is about 7–8 GB before the first project, then about 10.5–11.5 GB once a first project is built. AgentKnife itself is about 35 MB.
