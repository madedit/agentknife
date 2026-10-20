//! What is installed, compared with what AgentKnife needs. Read-only: never
//! installs, and never calls an AI.

use std::path::{Path, PathBuf};

use ak_types::doctor::{CheckItem, CheckScope, CheckStatus, DoctorReport};

use crate::probe::Probe;
use crate::toolchain::{Pinned, Toolchain};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    MacOs,
    Linux,
}

impl Platform {
    pub fn current() -> Platform {
        if cfg!(windows) {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::MacOs
        } else {
            Platform::Linux
        }
    }
}

/// The WebView2 runtime's EdgeUpdate client id.
const WEBVIEW2_GUID: &str = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";

/// Linux libraries Tauri needs, checked with pkg-config (any one name will do).
const LINUX_PACKAGES: &[(&str, &[&str])] = &[
    ("linux-webkit2gtk", &["webkit2gtk-4.1"]),
    ("linux-javascriptcore", &["javascriptcoregtk-4.1"]),
    ("linux-libsoup", &["libsoup-3.0"]),
    // Debian and Alpine ship Ayatana; Fedora, Arch and openSUSE the original.
    (
        "linux-appindicator",
        &["ayatana-appindicator3-0.1", "appindicator3-0.1"],
    ),
    ("linux-librsvg", &["librsvg-2.0"]),
];

/// Checks the isolated toolchain and the system dependencies for `platform`.
pub async fn check(
    toolchain: &Toolchain,
    pinned: &Pinned,
    platform: Platform,
    probe: &dyn Probe,
) -> DoctorReport {
    let mut items = isolated(toolchain, pinned, probe).await;
    items.extend(match platform {
        Platform::Windows => windows(probe).await,
        Platform::MacOs => macos(probe).await,
        Platform::Linux => linux(probe).await,
    });
    DoctorReport { items }
}

fn item(id: &str, scope: CheckScope, status: CheckStatus) -> CheckItem {
    CheckItem {
        id: id.into(),
        scope,
        status,
        found: None,
        wanted: None,
        detail: None,
    }
}

/// The first `1.2` / `1.2.3` / `v4.1.0`-like token.
pub fn parse_version(text: &str) -> Option<String> {
    text.split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | ',' | '"'))
        .map(|t| t.trim_start_matches('v'))
        .find(|t| {
            let parts: Vec<&str> = t.split('.').collect();
            parts.len() >= 2
                && parts
                    .iter()
                    .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        })
        .map(str::to_owned)
}

/// Runs `--version`-style probes for a pinned tool.
async fn pinned_tool(
    id: &str,
    exe: &Path,
    args: &[&str],
    wanted: &str,
    env: &[(String, String)],
    probe: &dyn Probe,
) -> CheckItem {
    let mut it = item(id, CheckScope::Isolated, CheckStatus::Missing);
    it.wanted = Some(wanted.to_owned());
    if !probe.exists(exe) {
        return it;
    }
    let program = exe.to_string_lossy();
    let Some(out) = probe.run(&program, args, env).await else {
        it.status = CheckStatus::Error;
        it.detail = Some(format!("{program} did not run"));
        return it;
    };
    let text = format!("{}\n{}", out.stdout, out.stderr);
    match parse_version(&text) {
        Some(v) => {
            it.status = if v == wanted {
                CheckStatus::Ok
            } else {
                CheckStatus::WrongVersion
            };
            it.found = Some(v);
        }
        None => {
            it.status = CheckStatus::Error;
            it.detail = Some(text.trim().chars().take(500).collect());
        }
    }
    it
}

async fn isolated(tc: &Toolchain, pinned: &Pinned, probe: &dyn Probe) -> Vec<CheckItem> {
    let env = tc.env();
    let cargo_bin = tc.cargo_bin();
    let mut items = vec![
        pinned_tool(
            "rust",
            &tc.exe(&cargo_bin, "rustc"),
            &["--version"],
            &pinned.rust,
            &env,
            probe,
        )
        .await,
    ];

    // The wasm32 target, via rustup for the pinned toolchain.
    let mut wasm = item("wasm32-target", CheckScope::Isolated, CheckStatus::Missing);
    let rustup = tc.exe(&cargo_bin, "rustup");
    if items[0].status == CheckStatus::Ok && probe.exists(&rustup) {
        let program = rustup.to_string_lossy();
        match probe
            .run(&program, &["target", "list", "--installed"], &env)
            .await
        {
            Some(out)
                if out
                    .stdout
                    .lines()
                    .any(|l| l.trim() == "wasm32-unknown-unknown") =>
            {
                wasm.status = CheckStatus::Ok;
            }
            Some(_) => {}
            None => {
                wasm.status = CheckStatus::Error;
                wasm.detail = Some("rustup did not run".into());
            }
        }
    }
    items.push(wasm);

    items.push(
        pinned_tool(
            "trunk",
            &tc.exe(&cargo_bin, "trunk"),
            &["--version"],
            &pinned.trunk,
            &env,
            probe,
        )
        .await,
    );
    items.push(
        pinned_tool(
            "wasm-bindgen",
            &tc.exe(&cargo_bin, "wasm-bindgen"),
            &["--version"],
            &pinned.wasm_bindgen,
            &env,
            probe,
        )
        .await,
    );
    items.push(
        pinned_tool(
            "tauri-cli",
            &tc.exe(&cargo_bin, "cargo-tauri"),
            &["--version"],
            &pinned.tauri_cli,
            &env,
            probe,
        )
        .await,
    );
    items.push(
        pinned_tool(
            "tailwind",
            &tc.exe(&tc.bin(), "tailwindcss"),
            &["--help"],
            &pinned.tailwind,
            &env,
            probe,
        )
        .await,
    );
    items
}

pub(crate) fn program_files_x86(probe: &dyn Probe) -> PathBuf {
    PathBuf::from(
        probe
            .env_var("ProgramFiles(x86)")
            .unwrap_or_else(|| r"C:\Program Files (x86)".into()),
    )
}

async fn windows(probe: &dyn Probe) -> Vec<CheckItem> {
    let pf86 = program_files_x86(probe);

    // MSVC: Visual Studio / Build Tools with the C++ x64 tools component.
    let mut msvc = item("msvc-build-tools", CheckScope::System, CheckStatus::Missing);
    let vswhere = pf86
        .join("Microsoft Visual Studio")
        .join("Installer")
        .join("vswhere.exe");
    if probe.exists(&vswhere) {
        let args = [
            "-products",
            "*",
            "-requires",
            "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
            "-latest",
            "-property",
            "installationVersion",
        ];
        match probe.run(&vswhere.to_string_lossy(), &args, &[]).await {
            Some(out) if out.success && parse_version(&out.stdout).is_some() => {
                msvc.status = CheckStatus::Ok;
                msvc.found = parse_version(&out.stdout);
            }
            Some(_) => {}
            None => {
                msvc.status = CheckStatus::Error;
                msvc.detail = Some("vswhere did not run".into());
            }
        }
    }

    // Windows SDK: a 10.0.x Lib folder with the x64 import libraries.
    let mut sdk = item("windows-sdk", CheckScope::System, CheckStatus::Missing);
    let lib = pf86.join("Windows Kits").join("10").join("Lib");
    let mut versions: Vec<String> = probe
        .list_dir(&lib)
        .into_iter()
        .filter(|v| v.starts_with("10.0."))
        .filter(|v| probe.exists(&lib.join(v).join("um").join("x64").join("kernel32.lib")))
        .collect();
    versions.sort_by_key(|v| version_key(v));
    if let Some(v) = versions.pop() {
        sdk.status = CheckStatus::Ok;
        sdk.found = Some(v);
    }

    // WebView2 runtime (built into Windows 11; may be missing on Windows 10).
    let mut webview = item("webview2", CheckScope::System, CheckStatus::Missing);
    for key in [
        format!(r"HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{WEBVIEW2_GUID}"),
        format!(r"HKLM\SOFTWARE\Microsoft\EdgeUpdate\Clients\{WEBVIEW2_GUID}"),
        format!(r"HKCU\Software\Microsoft\EdgeUpdate\Clients\{WEBVIEW2_GUID}"),
    ] {
        let Some(out) = probe.run("reg", &["query", &key, "/v", "pv"], &[]).await else {
            continue;
        };
        let pv = out
            .stdout
            .lines()
            .find(|l| l.trim_start().starts_with("pv"))
            .and_then(parse_version);
        if let Some(v) = pv.filter(|v| v != "0.0.0.0") {
            webview.status = CheckStatus::Ok;
            webview.found = Some(v);
            break;
        }
    }
    if webview.status != CheckStatus::Ok {
        let app = pf86
            .join("Microsoft")
            .join("EdgeWebView")
            .join("Application");
        if let Some(v) = probe
            .list_dir(&app)
            .into_iter()
            .filter(|d| parse_version(d).as_deref() == Some(d.as_str()))
            .max_by_key(|v| version_key(v))
        {
            webview.status = CheckStatus::Ok;
            webview.found = Some(v);
        }
    }
    vec![msvc, sdk, webview]
}

async fn macos(probe: &dyn Probe) -> Vec<CheckItem> {
    let mut clt = item("xcode-clt", CheckScope::System, CheckStatus::Missing);
    match probe.run("xcode-select", &["-p"], &[]).await {
        Some(out) if out.success && !out.stdout.trim().is_empty() => {
            clt.status = CheckStatus::Ok;
            clt.detail = Some(out.stdout.trim().to_owned());
            if let Some(info) = probe
                .run(
                    "pkgutil",
                    &["--pkg-info=com.apple.pkg.CLTools_Executables"],
                    &[],
                )
                .await
            {
                clt.found = info
                    .stdout
                    .lines()
                    .find(|l| l.starts_with("version:"))
                    .and_then(parse_version);
            }
        }
        Some(_) => {}
        None => {
            clt.status = CheckStatus::Error;
            clt.detail = Some("xcode-select did not run".into());
        }
    }
    vec![clt]
}

async fn linux(probe: &dyn Probe) -> Vec<CheckItem> {
    let mut items = Vec::new();
    let has_pkg_config = probe.run("pkg-config", &["--version"], &[]).await.is_some();
    for (id, names) in LINUX_PACKAGES {
        let mut it = item(id, CheckScope::System, CheckStatus::Missing);
        if !has_pkg_config {
            it.detail = Some("pkg-config is not installed".into());
            items.push(it);
            continue;
        }
        for pkg in *names {
            if let Some(out) = probe.run("pkg-config", &["--modversion", pkg], &[]).await
                && out.success
            {
                it.status = CheckStatus::Ok;
                it.found = parse_version(&out.stdout);
                break;
            }
        }
        items.push(it);
    }
    let mut cc = item("linux-c-compiler", CheckScope::System, CheckStatus::Missing);
    if let Some(out) = probe.run("cc", &["--version"], &[]).await
        && out.success
    {
        cc.status = CheckStatus::Ok;
        cc.found = parse_version(&out.stdout);
    }
    items.push(cc);
    items
}

/// Numeric ordering for dotted versions (`10.0.22621.0` > `10.0.19041.0`).
fn version_key(v: &str) -> Vec<u64> {
    v.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}
