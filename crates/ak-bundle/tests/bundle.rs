//! The bundler against a fake `cargo tauri build` (src/bin/fake_build.rs).

// Test code: clippy's allow-unwrap-in-tests does not cover helper fns here.
#![allow(clippy::unwrap_used)]

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ak_bundle::{BundleError, BundleEvent, BundleStage, Bundler};
use ak_preview::Tools;
use ak_project::{CheckpointKind, Timeline};
use ak_types::stack::{
    ArchiveFormat, BundleOutput, BundlePlan, BundleTarget, CommandSpec, HostOs, JsonField,
    PreviewMarker, ScanIn, Tool,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

const MARKERS: &[PreviewMarker] = &[
    PreviewMarker {
        scan: ScanIn::Program,
        text: "AK_BRIDGE_READY",
    },
    PreviewMarker {
        scan: ScanIn::Dir("dist"),
        text: "__ak_inspector_overlay",
    },
];

fn plan(mode: &str) -> BundlePlan {
    let command = CommandSpec {
        tool: Tool::CargoTauri,
        args: vec!["build".into()],
        cwd: "",
        env: vec![("FAKE_BUILD".into(), mode.into())],
        env_remove: Vec::new(),
    };
    let target = |os, output| BundleTarget {
        os,
        command: command.clone(),
        output,
    };
    BundlePlan {
        targets: vec![
            target(HostOs::Windows, BundleOutput::Archive(ArchiveFormat::Zip)),
            target(HostOs::Linux, BundleOutput::Archive(ArchiveFormat::TarGz)),
            target(HostOs::MacOs, BundleOutput::Archive(ArchiveFormat::TarGz)),
        ],
        program: "target/release/app",
        name: JsonField {
            file: "app.json",
            pointer: "/productName",
        },
        version: JsonField {
            file: "app.json",
            pointer: "/version",
        },
        preview_markers: MARKERS,
    }
}

fn tools() -> Tools {
    let fake = PathBuf::from(env!("CARGO_BIN_EXE_ak-bundle-fake-build"));
    Tools {
        cargo: fake.clone(),
        trunk: fake.clone(),
        cargo_tauri: fake,
        env: Vec::new(),
        path_prefix: Vec::new(),
    }
}

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("app.json"),
        r#"{"productName": "My Todos", "version": "1.2.0"}"#,
    )
    .unwrap();
    std::fs::write(dir.path().join(".gitignore"), "/target/\n/dist/\n").unwrap();
    dir
}

async fn bundle(
    root: &Path,
    mode: &str,
    cancel: &CancellationToken,
) -> (Result<ak_bundle::Bundle, BundleError>, Vec<BundleEvent>) {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let result = Bundler::new(root, plan(mode), tools())
        .build(&tx, cancel)
        .await;
    drop(tx);
    let mut events = Vec::new();
    while let Some(e) = rx.recv().await {
        events.push(e);
    }
    (result, events)
}

/// The archive's single entry: (path inside, contents, unix mode).
fn read_archive(path: &Path) -> (String, Vec<u8>, u32) {
    let file = std::fs::File::open(path).unwrap();
    if path.to_string_lossy().ends_with(".zip") {
        let mut zip = zip::ZipArchive::new(file).unwrap();
        assert_eq!(zip.len(), 1);
        let mut entry = zip.by_index(0).unwrap();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        (
            entry.name().unwrap().into_owned(),
            data,
            entry.unix_mode().unwrap_or(0) & 0o777,
        )
    } else {
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
        let mut entries = tar.entries().unwrap();
        let mut entry = entries.next().unwrap().unwrap();
        let name = entry.path().unwrap().to_string_lossy().into_owned();
        let mode = entry.header().mode().unwrap();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        assert!(entries.next().is_none());
        (name, data, mode & 0o777)
    }
}

#[tokio::test]
async fn builds_packs_and_records_a_checkpoint() {
    let dir = project();
    let (result, events) = bundle(dir.path(), "ok", &CancellationToken::new()).await;
    let b = result.unwrap();

    let ext = if cfg!(windows) { "zip" } else { "tar.gz" };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    };
    assert_eq!(
        b.path,
        dir.path()
            .join("target/release/bundle")
            .join(ext)
            .join(format!("My Todos_1.2.0_{arch}.{ext}"))
    );
    let (inner, data, mode) = read_archive(&b.path);
    let exe = std::env::consts::EXE_SUFFIX;
    assert_eq!(inner, format!("My Todos/My Todos{exe}"));
    assert_eq!(data, b"\x7fELF...the app...");
    if ext == "tar.gz" {
        // Runs after unpacking (zip is for Windows, which has no such bit).
        assert_eq!(mode, 0o755);
    }

    let stages: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            BundleEvent::Stage(s) => Some(*s),
            BundleEvent::Output(_) => None,
        })
        .collect();
    assert_eq!(
        stages,
        [
            BundleStage::Building,
            BundleStage::Checking,
            BundleStage::Packing
        ]
    );
    // Output lines arrive without colors, stderr included.
    assert!(events.contains(&BundleEvent::Output("    Building [=====>   ] 5/9".into())));

    let timeline = Timeline::open(dir.path()).unwrap();
    let last = timeline.entries().last().unwrap();
    assert_eq!(last.kind, CheckpointKind::Bundle);
    assert_eq!(Some(&last.id), b.checkpoint.as_ref());
    assert!(last.label.contains("My Todos_1.2.0"));

    // Again: replaces the archive, records another bundle entry.
    let (again, _) = bundle(dir.path(), "ok", &CancellationToken::new()).await;
    assert_eq!(again.unwrap().path, b.path);
    assert_eq!(
        Timeline::open(dir.path())
            .unwrap()
            .entries()
            .iter()
            .filter(|e| e.kind == CheckpointKind::Bundle)
            .count(),
        2
    );
}

#[tokio::test]
async fn a_failed_build_returns_the_compiler_output() {
    let dir = project();
    let (result, _) = bundle(dir.path(), "fail", &CancellationToken::new()).await;
    match result {
        Err(BundleError::BuildFailed { output }) => {
            assert!(output.contains("error[E0308]"), "{output}");
            assert!(output.contains("src-tauri/src/main.rs:9:5"), "{output}");
        }
        other => panic!("{other:?}"),
    }
    assert!(!dir.path().join("target/release/bundle").exists());
}

#[tokio::test]
async fn preview_code_in_the_build_is_refused() {
    let dir = project();
    let (result, _) = bundle(dir.path(), "leak", &CancellationToken::new()).await;
    match result {
        Err(BundleError::PreviewCode { marker, .. }) => assert_eq!(marker, "AK_BRIDGE_READY"),
        other => panic!("{other:?}"),
    }

    // The screens too.
    let dir = project();
    std::fs::create_dir_all(dir.path().join("dist/assets")).unwrap();
    std::fs::write(
        dir.path().join("dist/assets/app_bg.wasm"),
        "..__ak_inspector_overlay..",
    )
    .unwrap();
    // A fresh `ok` build rewrites only dist/index.html; the stale wasm stays.
    let (result, _) = bundle(dir.path(), "ok", &CancellationToken::new()).await;
    match result {
        Err(BundleError::PreviewCode { marker, file }) => {
            assert_eq!(marker, "__ak_inspector_overlay");
            assert!(file.ends_with("app_bg.wasm"), "{file:?}");
        }
        other => panic!("{other:?}"),
    }
    assert!(!dir.path().join("target/release/bundle").exists());
}

#[tokio::test]
async fn missing_program_or_app_info_is_an_error() {
    let dir = project();
    let (result, _) = bundle(dir.path(), "none", &CancellationToken::new()).await;
    assert!(matches!(result, Err(BundleError::Missing(_))), "{result:?}");

    let dir = project();
    std::fs::write(dir.path().join("app.json"), r#"{"productName": ""}"#).unwrap();
    let (result, events) = bundle(dir.path(), "ok", &CancellationToken::new()).await;
    assert!(
        matches!(result, Err(BundleError::AppInfo { what: "name", .. })),
        "{result:?}"
    );
    assert!(events.is_empty(), "checked before building");
}

#[tokio::test]
async fn cancelling_stops_the_build() {
    let dir = project();
    let cancel = CancellationToken::new();
    let c = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(500)).await;
        c.cancel();
    });
    let started = std::time::Instant::now();
    let (result, _) = bundle(dir.path(), "slow", &cancel).await;
    assert!(matches!(result, Err(BundleError::Cancelled)), "{result:?}");
    assert!(started.elapsed() < Duration::from_secs(20));
}
