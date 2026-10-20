//! What to tell the user once a bundle is ready: how to install and run it on
//! their computer, how to find the file, and why other systems need their own
//! build. Steps are fluent ids (`bundle-guide-…`) with the arguments `$file`
//! and `$app` ([`guide_args`]).

use std::path::Path;

use ak_types::doctor::GuideStep;
use ak_types::stack::{ArchiveFormat, BundleOutput, HostOs};

use crate::Bundle;

fn step(message: &str) -> GuideStep {
    GuideStep {
        message: message.to_owned(),
        command: None,
        link: None,
    }
}

fn command(message: &str, command: &str) -> GuideStep {
    GuideStep {
        command: Some(command.to_owned()),
        ..step(message)
    }
}

/// Plain-language steps for installing and starting a bundle made on `os`.
pub fn install_guide(os: HostOs, output: BundleOutput) -> Vec<GuideStep> {
    let mut steps: Vec<GuideStep> = match (os, output) {
        (HostOs::Windows, BundleOutput::Archive(ArchiveFormat::Zip)) => [
            "bundle-guide-win-extract",
            "bundle-guide-win-move",
            "bundle-guide-win-run",
            "bundle-guide-win-smartscreen",
            "bundle-guide-win-shortcut",
        ]
        .map(step)
        .into(),
        (HostOs::MacOs, BundleOutput::Installer { kind: "dmg", .. }) => [
            "bundle-guide-mac-open",
            "bundle-guide-mac-drag",
            "bundle-guide-mac-eject",
            "bundle-guide-mac-first-open",
            "bundle-guide-mac-settings",
        ]
        .map(step)
        .into(),
        (HostOs::Linux, BundleOutput::Archive(ArchiveFormat::TarGz)) => vec![
            step("bundle-guide-linux-extract"),
            command(
                "bundle-guide-linux-libraries",
                "sudo apt install libwebkit2gtk-4.1-0",
            ),
            step("bundle-guide-linux-run"),
        ],
        _ => vec![step("bundle-guide-open")],
    };
    steps.push(step(match os {
        HostOs::Windows => "bundle-guide-share-windows",
        HostOs::MacOs => "bundle-guide-share-macos",
        HostOs::Linux => "bundle-guide-share-linux",
    }));
    steps
}

/// `$file` and `$app` for the guide's messages.
pub fn guide_args(bundle: &Bundle) -> [(&'static str, String); 2] {
    let file = bundle
        .path
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default();
    [("file", file), ("app", bundle.app.clone())]
}

/// Fluent id of an OS's name (`os-windows`).
pub fn os_message(os: HostOs) -> &'static str {
    match os {
        HostOs::Windows => "os-windows",
        HostOs::MacOs => "os-macos",
        HostOs::Linux => "os-linux",
    }
}

/// When the user wants the app for another system: fluent id
/// `bundle-other-os`, with `$host` and `$requested` as `os-*` names to fill in.
/// `None` when `requested` is this computer's system.
pub fn other_os_note(requested: HostOs, host: HostOs) -> Option<&'static str> {
    (requested != host).then_some("bundle-other-os")
}

/// Shows the file in the system's file manager (Explorer, Finder, or the
/// folder on Linux). Returns once the file manager was started.
pub fn reveal(path: &Path) -> std::io::Result<()> {
    let os = HostOs::current().ok_or_else(|| std::io::Error::other("unsupported system"))?;
    // Explorer exits with 1 even when it worked: only starting it counts.
    reveal_command(path, os).spawn().map(drop)
}

fn reveal_command(path: &Path, os: HostOs) -> std::process::Command {
    match os {
        HostOs::Windows => {
            let mut c = std::process::Command::new("explorer.exe");
            #[cfg(windows)]
            {
                // Explorer reads `/select,"path"` itself; Rust's quoting of a
                // single argument with spaces would break it.
                use std::os::windows::process::CommandExt;
                // `/` separators (from plan paths) make it open the wrong folder.
                let path = path.to_string_lossy().replace('/', "\\");
                c.raw_arg(format!("/select,\"{path}\""));
            }
            #[cfg(not(windows))]
            c.arg(format!("/select,{}", path.display()));
            c
        }
        HostOs::MacOs => {
            let mut c = std::process::Command::new("open");
            c.arg("-R").arg(path);
            c
        }
        HostOs::Linux => {
            let mut c = std::process::Command::new("xdg-open");
            c.arg(path.parent().unwrap_or(path));
            c
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(steps: &[GuideStep]) -> Vec<&str> {
        steps.iter().map(|s| s.message.as_str()).collect()
    }

    #[test]
    fn each_system_gets_its_own_steps() {
        let win = install_guide(HostOs::Windows, BundleOutput::Archive(ArchiveFormat::Zip));
        assert!(ids(&win).contains(&"bundle-guide-win-smartscreen"));
        assert_eq!(ids(&win).last(), Some(&"bundle-guide-share-windows"));

        let dmg = BundleOutput::Installer {
            kind: "dmg",
            dir: "d",
            extension: "dmg",
        };
        let mac = install_guide(HostOs::MacOs, dmg);
        assert!(ids(&mac).contains(&"bundle-guide-mac-first-open"));
        assert!(ids(&mac).contains(&"bundle-guide-mac-settings"));

        let linux = install_guide(HostOs::Linux, BundleOutput::Archive(ArchiveFormat::TarGz));
        assert!(linux.iter().any(|s| {
            s.command
                .as_deref()
                .is_some_and(|c| c.contains("libwebkit2gtk-4.1"))
        }));

        // Something new: still a usable answer.
        let other = install_guide(HostOs::Linux, BundleOutput::Archive(ArchiveFormat::Zip));
        assert_eq!(
            ids(&other),
            ["bundle-guide-open", "bundle-guide-share-linux"]
        );
    }

    #[test]
    fn other_systems_need_their_own_computer() {
        assert_eq!(
            other_os_note(HostOs::MacOs, HostOs::Windows),
            Some("bundle-other-os")
        );
        assert_eq!(other_os_note(HostOs::Windows, HostOs::Windows), None);
    }

    #[test]
    fn reveal_selects_the_file() {
        let path = Path::new("/p/My App/out.zip");
        let mac = reveal_command(path, HostOs::MacOs);
        assert_eq!(mac.get_program(), "open");
        assert_eq!(
            mac.get_args().collect::<Vec<_>>(),
            ["-R", "/p/My App/out.zip"]
        );
        let linux = reveal_command(path, HostOs::Linux);
        assert_eq!(linux.get_program(), "xdg-open");
        assert_eq!(linux.get_args().collect::<Vec<_>>(), ["/p/My App"]);
        assert_eq!(
            reveal_command(path, HostOs::Windows).get_program(),
            "explorer.exe"
        );
    }
}
