//! System-dependency guides and installer actions, against canned answers.

// Test code: clippy's allow-unwrap-in-tests does not cover helper fns here.
#![allow(clippy::unwrap_used)]

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ak_doctor::Platform;
use ak_doctor::probe::{Probe, ProbeOutput};
use ak_doctor::system::{
    BUILD_TOOLS_PAGE, Distro, WEBVIEW2_PAGE, action_command, guides, launch, linux_command,
    parse_os_release,
};
use ak_types::doctor::{
    CheckItem, CheckScope, CheckStatus, DoctorReport, SystemAction, SystemGuide,
};
use ak_types::i18n::{I18n, Locale};
use futures_util::future::BoxFuture;

#[derive(Default)]
struct Fake {
    /// Programs that start and succeed.
    programs: HashSet<String>,
    paths: HashSet<PathBuf>,
    files: HashMap<PathBuf, String>,
    calls: Mutex<Vec<(String, Vec<String>)>>,
}

impl Fake {
    fn program(mut self, name: &str) -> Self {
        self.programs.insert(name.into());
        self
    }

    fn os_release(mut self, text: &str) -> Self {
        self.files
            .insert(PathBuf::from("/etc/os-release"), text.into());
        self
    }
}

impl Probe for Fake {
    fn run<'a>(
        &'a self,
        program: &'a str,
        args: &'a [&'a str],
        _env: &'a [(String, String)],
    ) -> BoxFuture<'a, Option<ProbeOutput>> {
        self.calls.lock().unwrap().push((
            program.to_owned(),
            args.iter().map(|a| a.to_string()).collect(),
        ));
        let out = self.programs.contains(program).then(|| ProbeOutput {
            success: true,
            stdout: String::new(),
            stderr: String::new(),
        });
        Box::pin(async move { out })
    }

    fn exists(&self, path: &Path) -> bool {
        self.paths.contains(path)
    }

    fn list_dir(&self, _path: &Path) -> Vec<String> {
        Vec::new()
    }

    fn env_var(&self, name: &str) -> Option<String> {
        (name == "ProgramFiles(x86)").then(|| "PF86".into())
    }

    fn read_file(&self, path: &Path) -> Option<String> {
        self.files.get(path).cloned()
    }
}

fn report(missing: &[&str], ok: &[&str]) -> DoctorReport {
    let item = |id: &&str, status| CheckItem {
        id: (*id).into(),
        scope: CheckScope::System,
        status,
        found: None,
        wanted: None,
        detail: None,
    };
    DoctorReport {
        items: missing
            .iter()
            .map(|id| item(id, CheckStatus::Missing))
            .chain(ok.iter().map(|id| item(id, CheckStatus::Ok)))
            .collect(),
    }
}

const WINDOWS: [&str; 3] = ["msvc-build-tools", "windows-sdk", "webview2"];

#[tokio::test]
async fn windows_with_winget_offers_official_installers() {
    let probe = Fake::default().program("winget");
    let g = guides(&report(&WINDOWS, &[]), Platform::Windows, &probe).await;
    assert_eq!(g.len(), 2, "the SDK comes with the C++ workload");
    assert_eq!(g[0].item, "msvc-build-tools");
    assert_eq!(g[0].covers, vec!["msvc-build-tools", "windows-sdk"]);
    assert_eq!(g[0].action, Some(SystemAction::WingetBuildTools));
    assert_eq!(g[1].action, Some(SystemAction::WingetWebView2));
    assert_eq!(
        g[0].steps.last().unwrap().message,
        "doctor-guide-check-again"
    );

    let (program, args) = action_command(&SystemAction::WingetBuildTools, &probe);
    assert_eq!(program, "winget");
    assert!(
        args.windows(2)
            .any(|w| w == ["--id", "Microsoft.VisualStudio.BuildTools"])
    );
    assert!(args.contains(&"--exact".to_owned()));
    let over = args.iter().position(|a| a == "--override").unwrap();
    assert!(args[over + 1].contains("Microsoft.VisualStudio.Workload.VCTools"));
    assert!(args[over + 1].contains("--includeRecommended"));
}

#[tokio::test]
async fn windows_without_winget_opens_the_download_pages() {
    let g = guides(&report(&WINDOWS, &[]), Platform::Windows, &Fake::default()).await;
    assert_eq!(
        g[0].action,
        Some(SystemAction::OpenPage {
            url: BUILD_TOOLS_PAGE.into()
        })
    );
    assert!(
        g[0].steps
            .iter()
            .any(|s| s.link.as_deref() == Some(BUILD_TOOLS_PAGE))
    );
    assert_eq!(
        g[1].action,
        Some(SystemAction::OpenPage {
            url: WEBVIEW2_PAGE.into()
        })
    );
}

#[tokio::test]
async fn windows_sdk_alone_goes_through_the_vs_installer() {
    let installer = PathBuf::from("PF86")
        .join("Microsoft Visual Studio")
        .join("Installer")
        .join("setup.exe");
    let mut probe = Fake::default().program("winget");
    probe.paths.insert(installer.clone());
    let r = report(&["windows-sdk"], &["msvc-build-tools", "webview2"]);
    let g = guides(&r, Platform::Windows, &probe).await;
    assert_eq!(g.len(), 1);
    assert_eq!(g[0].action, Some(SystemAction::OpenVsInstaller));
    let (_, args) = action_command(&SystemAction::OpenVsInstaller, &probe);
    assert_eq!(args, vec![installer.to_string_lossy().into_owned()]);

    // Nothing missing: nothing to explain.
    let g = guides(&report(&[], &WINDOWS), Platform::Windows, &probe).await;
    assert!(g.is_empty());
}

#[tokio::test]
async fn macos_uses_apples_installer() {
    let g = guides(
        &report(&["xcode-clt"], &[]),
        Platform::MacOs,
        &Fake::default(),
    )
    .await;
    assert_eq!(g[0].action, Some(SystemAction::XcodeInstall));
    let (program, args) = action_command(&SystemAction::XcodeInstall, &Fake::default());
    assert_eq!(
        (program.as_str(), args),
        ("xcode-select", vec!["--install".to_owned()])
    );
}

#[test]
fn os_release_names_the_package_manager() {
    let cases = [
        ("ID=ubuntu\nID_LIKE=debian\n", Some(Distro::Debian)),
        (
            "ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n",
            Some(Distro::Debian),
        ),
        ("ID=debian\n", Some(Distro::Debian)),
        ("ID=fedora\n", Some(Distro::Fedora)),
        ("ID=manjaro\nID_LIKE=arch\n", Some(Distro::Arch)),
        (
            "ID=\"opensuse-tumbleweed\"\nID_LIKE=\"opensuse suse\"\n",
            Some(Distro::OpenSuse),
        ),
        ("ID=alpine\n", Some(Distro::Alpine)),
        ("ID=gentoo\n", Some(Distro::Gentoo)),
        ("ID=nixos\n", None),
        // VERSION_ID must not be read as ID.
        ("VERSION_ID=\"24.04\"\nID=ubuntu\n", Some(Distro::Debian)),
    ];
    for (text, want) in cases {
        assert_eq!(parse_os_release(text), want, "{text}");
    }
}

#[tokio::test]
async fn linux_gives_one_command_for_all_packages() {
    let r = report(
        &["linux-webkit2gtk", "linux-c-compiler"],
        &["linux-libsoup"],
    );
    let probe = Fake::default().os_release("NAME=\"Ubuntu\"\nID=ubuntu\nID_LIKE=debian\n");
    let g = guides(&r, Platform::Linux, &probe).await;
    assert_eq!(g.len(), 1);
    assert_eq!(g[0].covers, vec!["linux-webkit2gtk", "linux-c-compiler"]);
    assert_eq!(g[0].action, None, "sudo is the user's to type");
    let cmd = g[0].steps.iter().find_map(|s| s.command.clone()).unwrap();
    assert_eq!(cmd, linux_command(Distro::Debian));
    assert!(cmd.contains("libwebkit2gtk-4.1-dev") && cmd.contains("pkg-config"));

    // Unknown distribution: the Tauri page instead of a guess.
    let g = guides(
        &r,
        Platform::Linux,
        &Fake::default().os_release("ID=nixos\n"),
    )
    .await;
    assert!(g[0].steps.iter().all(|s| s.command.is_none()));
    assert!(g[0].steps.iter().any(|s| s.link.is_some()));
}

#[tokio::test]
async fn launch_runs_the_installer_and_only_opens_official_pages() {
    let probe = Fake::default();
    let runner = Fake::default().program("winget");
    launch(&SystemAction::WingetWebView2, &probe, &runner)
        .await
        .unwrap();
    let calls = runner.calls.lock().unwrap().clone();
    assert_eq!(calls[0].0, "winget");
    assert!(
        calls[0]
            .1
            .contains(&"Microsoft.EdgeWebView2Runtime".to_owned())
    );

    let evil = SystemAction::OpenPage {
        url: "https://example.com/setup.exe".into(),
    };
    let err = launch(&evil, &probe, &runner).await.unwrap_err();
    assert!(err.contains("not an official"), "{err}");
    assert_eq!(runner.calls.lock().unwrap().len(), 1, "nothing was started");

    // winget missing or failing: the error is reported, not hidden.
    let err = launch(&SystemAction::WingetBuildTools, &probe, &Fake::default())
        .await
        .unwrap_err();
    assert!(err.contains("could not start"), "{err}");
}

#[tokio::test]
async fn every_guide_string_is_translated() {
    let mut all: Vec<SystemGuide> = Vec::new();
    for winget in [true, false] {
        let mut probe = if winget {
            Fake::default().program("winget")
        } else {
            Fake::default()
        };
        probe.paths.insert(
            PathBuf::from("PF86")
                .join("Microsoft Visual Studio")
                .join("Installer")
                .join("setup.exe"),
        );
        all.extend(guides(&report(&WINDOWS, &[]), Platform::Windows, &probe).await);
        let sdk = report(&["windows-sdk"], &["msvc-build-tools"]);
        all.extend(guides(&sdk, Platform::Windows, &probe).await);
    }
    all.extend(
        guides(
            &report(&["xcode-clt"], &[]),
            Platform::MacOs,
            &Fake::default(),
        )
        .await,
    );
    for os in ["ID=fedora\n", "ID=nixos\n"] {
        let probe = Fake::default().os_release(os);
        all.extend(guides(&report(&["linux-librsvg"], &[]), Platform::Linux, &probe).await);
    }
    let mut keys: Vec<String> = vec!["doctor-check-again".into()];
    for g in &all {
        keys.push(format!("doctor-item-{}", g.item));
        keys.extend(g.steps.iter().map(|s| s.message.clone()));
        if let Some(a) = &g.action {
            keys.push(format!("doctor-action-{}", a.key()));
            keys.push(format!("doctor-confirm-{}", a.key()));
        }
    }
    let actions = [
        "winget-build-tools",
        "winget-webview2",
        "open-vs-installer",
        "xcode-install",
        "open-page",
    ];
    for a in actions {
        assert!(
            keys.contains(&format!("doctor-action-{a}")),
            "{a} is never offered"
        );
    }
    for locale in Locale::ALL {
        let i18n = I18n::new(locale).unwrap();
        for key in &keys {
            assert_ne!(&i18n.t(key), key, "{locale} {key}");
        }
    }
}
