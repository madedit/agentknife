//! Step-by-step help for system dependencies AgentKnife cannot install into
//! its own folder. Where an official installer exists, `launch` starts it —
//! only after the user confirmed (the UI's job).

use std::path::{Path, PathBuf};

use ak_types::doctor::{CheckStatus, DoctorReport, GuideStep, SystemAction, SystemGuide};

use crate::detect::{Platform, program_files_x86};
use crate::probe::Probe;

pub const BUILD_TOOLS_PAGE: &str = "https://visualstudio.microsoft.com/visual-cpp-build-tools/";
pub const WEBVIEW2_PAGE: &str =
    "https://developer.microsoft.com/microsoft-edge/webview2/#download-section";
pub const TAURI_LINUX_PAGE: &str = "https://v2.tauri.app/start/prerequisites/#linux";

/// winget package ids (winget checks the installer against Microsoft's hash).
const WINGET_BUILD_TOOLS: &str = "Microsoft.VisualStudio.BuildTools";
const WINGET_WEBVIEW2: &str = "Microsoft.EdgeWebView2Runtime";
/// The C++ workload; "recommended" brings the Windows SDK.
const BUILD_TOOLS_OVERRIDE: &str = "--wait --passive --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended";

fn step(message: &str) -> GuideStep {
    GuideStep {
        message: message.into(),
        command: None,
        link: None,
    }
}

fn command(message: &str, command: &str) -> GuideStep {
    GuideStep {
        command: Some(command.into()),
        ..step(message)
    }
}

fn link(message: &str, url: &str) -> GuideStep {
    GuideStep {
        link: Some(url.into()),
        ..step(message)
    }
}

fn check_again() -> GuideStep {
    step("doctor-guide-check-again")
}

fn missing(report: &DoctorReport, id: &str) -> bool {
    report.item(id).is_some_and(|i| i.status != CheckStatus::Ok)
}

/// One guide per missing system item (Linux: one guide for all packages).
pub async fn guides(
    report: &DoctorReport,
    platform: Platform,
    probe: &dyn Probe,
) -> Vec<SystemGuide> {
    match platform {
        Platform::Windows => windows(report, probe).await,
        Platform::MacOs => macos(report),
        Platform::Linux => linux(report, probe),
    }
}

fn vs_installer(probe: &dyn Probe) -> PathBuf {
    program_files_x86(probe)
        .join("Microsoft Visual Studio")
        .join("Installer")
        .join("setup.exe")
}

async fn windows(report: &DoctorReport, probe: &dyn Probe) -> Vec<SystemGuide> {
    let winget = probe
        .run("winget", &["--version"], &[])
        .await
        .is_some_and(|o| o.success);
    let mut out = Vec::new();
    let msvc = missing(report, "msvc-build-tools");
    if msvc {
        let (steps, action) = if winget {
            (
                vec![
                    step("doctor-guide-msvc-what"),
                    step("doctor-guide-msvc-winget"),
                    step("doctor-guide-admin-prompt"),
                    check_again(),
                ],
                SystemAction::WingetBuildTools,
            )
        } else {
            (
                vec![
                    step("doctor-guide-msvc-what"),
                    link("doctor-guide-msvc-download", BUILD_TOOLS_PAGE),
                    step("doctor-guide-msvc-workload"),
                    check_again(),
                ],
                SystemAction::OpenPage {
                    url: BUILD_TOOLS_PAGE.into(),
                },
            )
        };
        // The C++ workload brings the Windows SDK with it.
        let mut covers = vec!["msvc-build-tools".to_owned()];
        if missing(report, "windows-sdk") {
            covers.push("windows-sdk".into());
        }
        out.push(SystemGuide {
            item: "msvc-build-tools".into(),
            covers,
            steps,
            action: Some(action),
        });
    } else if missing(report, "windows-sdk") {
        let installer = probe.exists(&vs_installer(probe));
        out.push(SystemGuide {
            item: "windows-sdk".into(),
            covers: vec!["windows-sdk".into()],
            steps: vec![
                step("doctor-guide-sdk-open-installer"),
                step("doctor-guide-sdk-modify"),
                step("doctor-guide-admin-prompt"),
                check_again(),
            ],
            action: installer.then_some(SystemAction::OpenVsInstaller),
        });
    }
    if missing(report, "webview2") {
        let (steps, action) = if winget {
            (
                vec![
                    step("doctor-guide-webview2-what"),
                    step("doctor-guide-webview2-winget"),
                    check_again(),
                ],
                SystemAction::WingetWebView2,
            )
        } else {
            (
                vec![
                    step("doctor-guide-webview2-what"),
                    link("doctor-guide-webview2-download", WEBVIEW2_PAGE),
                    check_again(),
                ],
                SystemAction::OpenPage {
                    url: WEBVIEW2_PAGE.into(),
                },
            )
        };
        out.push(SystemGuide {
            item: "webview2".into(),
            covers: vec!["webview2".into()],
            steps,
            action: Some(action),
        });
    }
    out
}

fn macos(report: &DoctorReport) -> Vec<SystemGuide> {
    if !missing(report, "xcode-clt") {
        return Vec::new();
    }
    vec![SystemGuide {
        item: "xcode-clt".into(),
        covers: vec!["xcode-clt".into()],
        steps: vec![
            step("doctor-guide-xcode-what"),
            step("doctor-guide-xcode-dialog"),
            step("doctor-guide-xcode-wait"),
            check_again(),
        ],
        action: Some(SystemAction::XcodeInstall),
    }]
}

/// Linux package managers, from `/etc/os-release`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Distro {
    Debian,
    Fedora,
    Arch,
    OpenSuse,
    Alpine,
    Gentoo,
}

/// Reads `ID` and `ID_LIKE` (Ubuntu, Mint and Pop!_OS are Debian-like, …).
pub fn parse_os_release(text: &str) -> Option<Distro> {
    let field = |key: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
            .map(|v| v.trim().trim_matches('"').to_ascii_lowercase())
            .unwrap_or_default()
    };
    let id = field("ID");
    let like = field("ID_LIKE");
    let ids = std::iter::once(id.as_str()).chain(like.split_whitespace());
    let mut found = None;
    for id in ids {
        found = match id {
            "debian" | "ubuntu" => Some(Distro::Debian),
            "fedora" => Some(Distro::Fedora),
            "arch" => Some(Distro::Arch),
            "suse" | "opensuse" => Some(Distro::OpenSuse),
            s if s.starts_with("opensuse") => Some(Distro::OpenSuse),
            "alpine" => Some(Distro::Alpine),
            "gentoo" => Some(Distro::Gentoo),
            _ => None,
        };
        if found.is_some() {
            break;
        }
    }
    found
}

/// Everything Tauri needs, in one command (from the Tauri prerequisites page,
/// plus pkg-config and the `-dev` packages AgentKnife's check looks for).
pub fn linux_command(distro: Distro) -> &'static str {
    match distro {
        Distro::Debian => {
            "sudo apt update && sudo apt install -y libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev pkg-config"
        }
        Distro::Fedora => {
            "sudo dnf install -y webkit2gtk4.1-devel openssl-devel curl wget file libappindicator-gtk3-devel librsvg2-devel libxdo-devel pkgconf-pkg-config && sudo dnf group install -y c-development"
        }
        Distro::Arch => {
            "sudo pacman -Syu --needed webkit2gtk-4.1 base-devel curl wget file openssl appmenu-gtk-module libappindicator-gtk3 librsvg xdotool"
        }
        Distro::OpenSuse => {
            "sudo zypper in -y webkit2gtk3-devel libopenssl-devel curl wget file libappindicator3-devel librsvg-devel && sudo zypper in -y -t pattern devel_basis"
        }
        Distro::Alpine => {
            "sudo apk add build-base webkit2gtk-4.1-dev curl wget file openssl libayatana-appindicator-dev librsvg-dev pkgconf"
        }
        Distro::Gentoo => {
            "sudo emerge --ask net-libs/webkit-gtk:4.1 dev-libs/libayatana-appindicator gnome-base/librsvg net-misc/curl net-misc/wget sys-apps/file"
        }
    }
}

fn linux(report: &DoctorReport, probe: &dyn Probe) -> Vec<SystemGuide> {
    let covers: Vec<String> = report
        .items
        .iter()
        .filter(|i| i.id.starts_with("linux-") && i.status != CheckStatus::Ok)
        .map(|i| i.id.clone())
        .collect();
    if covers.is_empty() {
        return Vec::new();
    }
    let distro = probe
        .read_file(Path::new("/etc/os-release"))
        .as_deref()
        .and_then(parse_os_release);
    let steps = match distro {
        Some(d) => vec![
            step("doctor-guide-linux-what"),
            step("doctor-guide-linux-terminal"),
            command("doctor-guide-linux-run", linux_command(d)),
            step("doctor-guide-linux-password"),
            check_again(),
        ],
        None => vec![
            step("doctor-guide-linux-what"),
            link("doctor-guide-linux-unknown", TAURI_LINUX_PAGE),
            check_again(),
        ],
    };
    vec![SystemGuide {
        item: "linux-packages".into(),
        covers,
        steps,
        // Needs sudo in a terminal: the user runs it, AgentKnife does not.
        action: None,
    }]
}

/// Program and arguments that start `action`.
pub fn action_command(action: &SystemAction, probe: &dyn Probe) -> (String, Vec<String>) {
    let winget = |id: &str, extra: &[&str]| {
        let mut args: Vec<String> = [
            "install",
            "--id",
            id,
            "--exact",
            "--source",
            "winget",
            "--accept-package-agreements",
            "--accept-source-agreements",
            "--disable-interactivity",
        ]
        .map(str::to_owned)
        .to_vec();
        args.extend(extra.iter().map(|s| (*s).to_owned()));
        ("winget".to_owned(), args)
    };
    match action {
        SystemAction::WingetBuildTools => {
            winget(WINGET_BUILD_TOOLS, &["--override", BUILD_TOOLS_OVERRIDE])
        }
        SystemAction::WingetWebView2 => winget(WINGET_WEBVIEW2, &[]),
        SystemAction::OpenVsInstaller => open(&vs_installer(probe).to_string_lossy()),
        SystemAction::XcodeInstall => ("xcode-select".into(), vec!["--install".into()]),
        SystemAction::OpenPage { url } => open(url),
    }
}

/// Opens a page or program the way a double-click would (so Windows can ask
/// for administrator rights when a program needs them).
fn open(target: &str) -> (String, Vec<String>) {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    (program.into(), vec![target.into()])
}

/// Starts `action`. winget runs to the end (minutes for Build Tools); the
/// others return once the installer or page is open. Afterwards the UI runs
/// `check` again.
pub async fn launch(
    action: &SystemAction,
    probe: &dyn Probe,
    runner: &dyn Probe,
) -> Result<(), String> {
    if let SystemAction::OpenPage { url } = action
        && !is_official_page(url)
    {
        return Err(format!("not an official download page: {url}"));
    }
    let (program, args) = action_command(action, probe);
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = runner
        .run(&program, &argv, &[])
        .await
        .ok_or_else(|| format!("{program} could not start"))?;
    // explorer exits with 1 even when it opened the target.
    let opened = program == "explorer";
    if out.success || opened {
        Ok(())
    } else {
        let text = format!("{}\n{}", out.stdout, out.stderr);
        Err(format!("{program} failed: {}", text.trim()))
    }
}

/// Pages a guide may open: only the ones above.
fn is_official_page(url: &str) -> bool {
    [BUILD_TOOLS_PAGE, WEBVIEW2_PAGE, TAURI_LINUX_PAGE].contains(&url)
}
