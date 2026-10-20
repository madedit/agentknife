//! Every install step and note renders in every language, with its arguments.

// Test code: clippy's allow-unwrap-in-tests does not cover helper fns here.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use ak_bundle::Bundle;
use ak_bundle::guide::{guide_args, install_guide, os_message, other_os_note};
use ak_types::i18n::{I18n, Locale, args};
use ak_types::stack::{ArchiveFormat, BundleOutput, HostOs};

const OUTPUTS: [BundleOutput; 3] = [
    BundleOutput::Archive(ArchiveFormat::Zip),
    BundleOutput::Archive(ArchiveFormat::TarGz),
    BundleOutput::Installer {
        kind: "dmg",
        dir: "target/release/bundle/dmg",
        extension: "dmg",
    },
];
const SYSTEMS: [HostOs; 3] = [HostOs::Windows, HostOs::MacOs, HostOs::Linux];

fn rendered(i18n: &I18n, id: &str, text: &str) {
    assert_ne!(text, id, "{id} missing in {}", i18n.locale());
    assert!(
        !text.contains('{'),
        "{id} in {}: unfilled argument: {text}",
        i18n.locale()
    );
}

#[test]
fn install_steps_render_with_the_file_and_app_names() {
    let bundle = Bundle {
        path: PathBuf::from("target/release/bundle/zip/My Todos_1.0.0_x64.zip"),
        output: OUTPUTS[0],
        app: "My Todos".into(),
        checkpoint: None,
    };
    for locale in Locale::ALL {
        let i18n = I18n::new(locale).unwrap();
        let a = args(guide_args(&bundle));
        for os in SYSTEMS {
            for output in OUTPUTS {
                for step in install_guide(os, output) {
                    let text = i18n.t_args(&step.message, &a);
                    rendered(&i18n, &step.message, &text);
                }
            }
        }
        let run = i18n.t_args("bundle-guide-win-run", &a);
        assert!(run.contains("My Todos"), "{run}");
        let done = i18n.t_args("bundle-done", &a);
        assert!(done.contains("My Todos_1.0.0_x64.zip"), "{done}");
        rendered(&i18n, "bundle-open-folder", &i18n.t("bundle-open-folder"));
    }
}

#[test]
fn the_other_os_note_names_both_systems() {
    let id = other_os_note(HostOs::MacOs, HostOs::Windows).unwrap();
    for locale in Locale::ALL {
        let i18n = I18n::new(locale).unwrap();
        let a = args([
            ("host", i18n.t(os_message(HostOs::Windows))),
            ("requested", i18n.t(os_message(HostOs::MacOs))),
        ]);
        let text = i18n.t_args(id, &a);
        rendered(&i18n, id, &text);
        assert!(text.contains("Windows") && text.contains("macOS"), "{text}");
    }
}
