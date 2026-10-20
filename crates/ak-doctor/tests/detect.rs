//! Doctor detection for every platform, against canned answers (no real tools).

// Test code: clippy's allow-unwrap-in-tests does not cover helper fns here.
#![allow(clippy::unwrap_used)]

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ak_doctor::probe::{Probe, ProbeOutput};
use ak_doctor::{Pinned, Platform, Toolchain, check, parse_version};
use ak_types::doctor::{CheckScope, CheckStatus, DoctorReport};
use futures_util::future::BoxFuture;

/// (command line, environment) of every run.
type Calls = Mutex<Vec<(String, Vec<(String, String)>)>>;

#[derive(Default)]
struct Fake {
    /// "program arg1 arg2" -> (success, stdout). Unknown commands cannot start.
    outputs: HashMap<String, (bool, String)>,
    paths: HashSet<PathBuf>,
    dirs: HashMap<PathBuf, Vec<String>>,
    env: HashMap<String, String>,
    calls: Calls,
}

impl Fake {
    fn out(mut self, cmd: impl Into<String>, stdout: &str) -> Self {
        self.outputs.insert(cmd.into(), (true, stdout.into()));
        self
    }

    /// The program exists but exits with an error (e.g. `xcode-select -p` without CLT).
    fn fail(mut self, cmd: impl Into<String>) -> Self {
        self.outputs.insert(cmd.into(), (false, String::new()));
        self
    }

    fn path(mut self, p: PathBuf) -> Self {
        self.paths.insert(p);
        self
    }

    fn exe(self, program: &Path, args: &str, stdout: &str) -> Self {
        let key = format!("{} {args}", program.to_string_lossy());
        self.path(program.to_path_buf()).out(key, stdout)
    }
}

impl Probe for Fake {
    fn run<'a>(
        &'a self,
        program: &'a str,
        args: &'a [&'a str],
        env: &'a [(String, String)],
    ) -> BoxFuture<'a, Option<ProbeOutput>> {
        let key = format!("{program} {}", args.join(" "));
        self.calls.lock().unwrap().push((key.clone(), env.to_vec()));
        let out = self.outputs.get(&key).map(|(ok, s)| ProbeOutput {
            success: *ok,
            stdout: s.clone(),
            stderr: String::new(),
        });
        Box::pin(async move { out })
    }

    fn exists(&self, path: &Path) -> bool {
        self.paths.contains(path)
    }

    fn list_dir(&self, path: &Path) -> Vec<String> {
        self.dirs.get(path).cloned().unwrap_or_default()
    }

    fn env_var(&self, name: &str) -> Option<String> {
        self.env.get(name).cloned()
    }
}

fn pinned() -> Pinned {
    Pinned {
        rust: "1.99.1".into(),
        rustup: "1.29.1".into(),
        downloads: Default::default(),
        trunk: "0.21.14".into(),
        wasm_bindgen: "0.2.129".into(),
        tauri_cli: "2.12.3".into(),
        tailwind: "4.3.3".into(),
    }
}

fn toolchain() -> Toolchain {
    Toolchain::new(PathBuf::from("ak").join("toolchain"), &pinned())
}

/// A fully installed isolated toolchain.
fn with_toolchain(f: Fake) -> Fake {
    let tc = toolchain();
    let cb = tc.cargo_bin();
    f.exe(
        &tc.exe(&cb, "rustc"),
        "--version",
        "rustc 1.99.1 (58cf8f9c3 2026-10-13)\n",
    )
    .exe(
        &tc.exe(&cb, "rustup"),
        "target list --installed",
        "wasm32-unknown-unknown\nx86_64-pc-windows-msvc\n",
    )
    .exe(&tc.exe(&cb, "trunk"), "--version", "trunk 0.21.14\n")
    .exe(
        &tc.exe(&cb, "wasm-bindgen"),
        "--version",
        "wasm-bindgen 0.2.129\n",
    )
    .exe(
        &tc.exe(&cb, "cargo-tauri"),
        "--version",
        "tauri-cli 2.12.3\n",
    )
    .exe(
        &tc.exe(&tc.bin(), "tailwindcss"),
        "--help",
        "≈ tailwindcss v4.3.3\n\nUsage: tailwindcss [--input input.css]\n",
    )
}

fn pf86() -> PathBuf {
    PathBuf::from("PF86")
}

fn with_windows_system(mut f: Fake) -> Fake {
    f.env.insert("ProgramFiles(x86)".into(), "PF86".into());
    let vswhere = pf86()
        .join("Microsoft Visual Studio")
        .join("Installer")
        .join("vswhere.exe");
    let lib = pf86().join("Windows Kits").join("10").join("Lib");
    f.dirs.insert(
        lib.clone(),
        vec!["10.0.19041.0".into(), "10.0.22621.0".into(), "wdf".into()],
    );
    let wv = r"reg query HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5} /v pv";
    f.exe(
        &vswhere,
        "-products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -latest -property installationVersion",
        "17.14.36310.24\r\n",
    )
    .path(lib.join("10.0.19041.0").join("um").join("x64").join("kernel32.lib"))
    .path(lib.join("10.0.22621.0").join("um").join("x64").join("kernel32.lib"))
    .out(
        wv,
        "\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}\r\n    pv    REG_SZ    141.0.3537.71\r\n\r\n",
    )
}

async fn report(platform: Platform, f: &Fake) -> DoctorReport {
    check(&toolchain(), &pinned(), platform, f).await
}

fn status(r: &DoctorReport, id: &str) -> CheckStatus {
    r.item(id).unwrap_or_else(|| panic!("no item {id}")).status
}

#[test]
fn version_parsing() {
    assert_eq!(
        parse_version("rustc 1.99.1 (58cf8f9c3 2026-10-13)").as_deref(),
        Some("1.99.1")
    );
    assert_eq!(
        parse_version("≈ tailwindcss v4.3.3").as_deref(),
        Some("4.3.3")
    );
    assert_eq!(parse_version("tauri-cli 2.12.3").as_deref(), Some("2.12.3"));
    assert_eq!(
        parse_version("pv    REG_SZ    141.0.3537.71").as_deref(),
        Some("141.0.3537.71")
    );
    assert_eq!(parse_version("error: no such command"), None);
}

#[tokio::test]
async fn windows_everything_installed() {
    let f = with_windows_system(with_toolchain(Fake::default()));
    let r = report(Platform::Windows, &f).await;
    assert!(r.ready(), "{r:#?}");
    let ids: Vec<&str> = r.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "rust",
            "wasm32-target",
            "trunk",
            "wasm-bindgen",
            "tauri-cli",
            "tailwind",
            "msvc-build-tools",
            "windows-sdk",
            "webview2"
        ]
    );
    assert_eq!(
        r.item("windows-sdk").unwrap().found.as_deref(),
        Some("10.0.22621.0"),
        "newest SDK"
    );
    assert_eq!(
        r.item("webview2").unwrap().found.as_deref(),
        Some("141.0.3537.71")
    );
    assert_eq!(r.item("tailwind").unwrap().found.as_deref(), Some("4.3.3"));
    assert!(
        r.items
            .iter()
            .filter(|i| i.scope == CheckScope::Isolated)
            .count()
            == 6
    );

    // Every toolchain probe ran with auto-install disabled and the pinned toolchain.
    let calls = f.calls.lock().unwrap();
    let rustc = calls.iter().find(|(k, _)| k.contains("rustc")).unwrap();
    assert!(
        rustc
            .1
            .contains(&("RUSTUP_AUTO_INSTALL".into(), "0".into()))
    );
    assert!(
        rustc
            .1
            .contains(&("RUSTUP_TOOLCHAIN".into(), "1.99.1".into()))
    );
}

#[tokio::test]
async fn nothing_installed_is_missing_not_error() {
    let r = report(Platform::Windows, &Fake::default()).await;
    assert!(!r.ready());
    assert!(
        r.items.iter().all(|i| i.status == CheckStatus::Missing),
        "{r:#?}"
    );
    assert_eq!(r.item("trunk").unwrap().wanted.as_deref(), Some("0.21.14"));
}

#[tokio::test]
async fn wrong_versions_and_unreadable_output() {
    let tc = toolchain();
    let cb = tc.cargo_bin();
    let mut f = with_windows_system(with_toolchain(Fake::default()));
    f.outputs.insert(
        format!("{} --version", tc.exe(&cb, "trunk").to_string_lossy()),
        (true, "trunk 0.20.0\n".into()),
    );
    f.outputs.insert(
        format!(
            "{} --version",
            tc.exe(&cb, "wasm-bindgen").to_string_lossy()
        ),
        (true, "segfault?\n".into()),
    );
    f.outputs.insert(
        format!("{} --version", tc.exe(&cb, "rustc").to_string_lossy()),
        (true, "rustc 1.98.0 (x)\n".into()),
    );
    let r = report(Platform::Windows, &f).await;
    let trunk = r.item("trunk").unwrap();
    assert_eq!(trunk.status, CheckStatus::WrongVersion);
    assert_eq!(
        (trunk.found.as_deref(), trunk.wanted.as_deref()),
        (Some("0.20.0"), Some("0.21.14"))
    );
    assert_eq!(status(&r, "wasm-bindgen"), CheckStatus::Error);
    assert!(
        r.item("wasm-bindgen")
            .unwrap()
            .detail
            .as_deref()
            .unwrap()
            .contains("segfault")
    );
    assert_eq!(status(&r, "rust"), CheckStatus::WrongVersion);
    assert_eq!(
        status(&r, "wasm32-target"),
        CheckStatus::Missing,
        "only checked on the pinned Rust"
    );
}

#[tokio::test]
async fn webview2_falls_back_to_the_install_folder() {
    let mut f = with_windows_system(Fake::default());
    let wv = r"reg query HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5} /v pv";
    f.outputs
        .insert(wv.into(), (true, "    pv    REG_SZ    0.0.0.0\r\n".into()));
    let app = pf86()
        .join("Microsoft")
        .join("EdgeWebView")
        .join("Application");
    f.dirs.insert(
        app,
        vec![
            "SetupMetrics".into(),
            "140.0.1.2".into(),
            "141.0.3537.71".into(),
        ],
    );
    let r = report(Platform::Windows, &f).await;
    assert_eq!(
        r.item("webview2").unwrap().found.as_deref(),
        Some("141.0.3537.71")
    );
}

#[tokio::test]
async fn macos_command_line_tools() {
    let f = with_toolchain(Fake::default())
        .out("xcode-select -p", "/Library/Developer/CommandLineTools\n")
        .out(
            "pkgutil --pkg-info=com.apple.pkg.CLTools_Executables",
            "package-id: com.apple.pkg.CLTools_Executables\nversion: 26.0.0.0.1.1757719676\n",
        );
    let r = report(Platform::MacOs, &f).await;
    assert!(r.ready(), "{r:#?}");
    assert_eq!(
        r.item("xcode-clt").unwrap().found.as_deref(),
        Some("26.0.0.0.1.1757719676")
    );
    let r = report(Platform::MacOs, &Fake::default().fail("xcode-select -p")).await;
    assert_eq!(status(&r, "xcode-clt"), CheckStatus::Missing);
}

#[tokio::test]
async fn linux_packages() {
    let f = with_toolchain(Fake::default())
        .out("pkg-config --version", "1.8.1\n")
        .out("pkg-config --modversion webkit2gtk-4.1", "2.48.1\n")
        .out("pkg-config --modversion javascriptcoregtk-4.1", "2.48.1\n")
        .out("pkg-config --modversion libsoup-3.0", "3.6.0\n")
        .out("pkg-config --modversion librsvg-2.0", "2.58.0\n")
        .out("cc --version", "cc (Ubuntu 14.2.0-4ubuntu2) 14.2.0\n");
    let r = report(Platform::Linux, &f).await;
    assert_eq!(status(&r, "linux-webkit2gtk"), CheckStatus::Ok);
    assert_eq!(status(&r, "linux-appindicator"), CheckStatus::Missing);
    assert_eq!(status(&r, "linux-c-compiler"), CheckStatus::Ok);
    assert!(!r.ready());

    // Fedora/Arch: the non-Ayatana appindicator counts too.
    let f = f.out("pkg-config --modversion appindicator3-0.1", "12.10.1\n");
    let r = report(Platform::Linux, &f).await;
    assert_eq!(status(&r, "linux-appindicator"), CheckStatus::Ok);

    let r = report(Platform::Linux, &with_toolchain(Fake::default())).await;
    let webkit = r.item("linux-webkit2gtk").unwrap();
    assert_eq!(webkit.status, CheckStatus::Missing);
    assert_eq!(
        webkit.detail.as_deref(),
        Some("pkg-config is not installed")
    );
}

#[test]
fn every_item_has_a_localized_name() {
    let ids = [
        "rust",
        "wasm32-target",
        "trunk",
        "wasm-bindgen",
        "tauri-cli",
        "tailwind",
        "msvc-build-tools",
        "windows-sdk",
        "webview2",
        "xcode-clt",
        "linux-webkit2gtk",
        "linux-javascriptcore",
        "linux-libsoup",
        "linux-appindicator",
        "linux-librsvg",
        "linux-c-compiler",
    ];
    for locale in ak_types::i18n::Locale::ALL {
        let i18n = ak_types::i18n::I18n::new(locale).unwrap();
        for id in ids {
            let key = format!("doctor-item-{id}");
            assert_ne!(i18n.t(&key), key, "{locale} {id}");
        }
    }
}
