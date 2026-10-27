//! The installer against in-memory downloads and fake programs (no network).

// Test code: clippy's allow-unwrap-in-tests does not cover helper fns here.
#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ak_doctor::assets::{self, Archive};
use ak_doctor::fetch::{Download, Fetch};
use ak_doctor::install::{Step, download, plan, rustup_init_args, unpack};
use ak_doctor::probe::{Probe, ProbeOutput};
use ak_doctor::{Installer, Pinned, Platform, Toolchain};
use ak_types::doctor::{CheckItem, CheckScope, CheckStatus, DoctorReport, InstallPhase};
use futures_util::StreamExt;
use futures_util::future::BoxFuture;
use sha2::{Digest, Sha256};

const WIN: &str = "x86_64-pc-windows-msvc";

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

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

/// Files by URL. Chunks of 4 bytes, so resume and progress have something to do.
#[derive(Default)]
struct FakeFetch {
    files: HashMap<String, Vec<u8>>,
    ignore_range: bool,
    calls: Mutex<Vec<(String, u64)>>,
}

impl Fetch for FakeFetch {
    fn get<'a>(&'a self, url: &'a str, from: u64) -> BoxFuture<'a, Result<Download, String>> {
        self.calls.lock().unwrap().push((url.to_owned(), from));
        let result = match self.files.get(url) {
            None => Err(format!("{url}: HTTP 404")),
            Some(data) => {
                let resumed = from > 0 && !self.ignore_range;
                let start = if resumed { from as usize } else { 0 };
                let chunks: Vec<Result<Vec<u8>, String>> =
                    data[start..].chunks(4).map(|c| Ok(c.to_vec())).collect();
                Ok(Download {
                    resumed,
                    size: Some(data.len() as u64),
                    body: futures_util::stream::iter(chunks).boxed(),
                })
            }
        };
        Box::pin(async move { result })
    }
}

type Calls = Mutex<Vec<(String, Vec<String>, Vec<(String, String)>)>>;

/// Nothing is installed; programs whose path ends with one of `succeed` exit 0,
/// the rest cannot start.
#[derive(Default)]
struct FakeProbe {
    succeed: Vec<String>,
    calls: Calls,
}

impl Probe for FakeProbe {
    fn run<'a>(
        &'a self,
        program: &'a str,
        args: &'a [&'a str],
        env: &'a [(String, String)],
    ) -> BoxFuture<'a, Option<ProbeOutput>> {
        self.calls.lock().unwrap().push((
            program.to_owned(),
            args.iter().map(|a| a.to_string()).collect(),
            env.to_vec(),
        ));
        let ok = self.succeed.iter().any(|s| program.ends_with(s.as_str()));
        let out = ok.then(|| ProbeOutput {
            success: true,
            stdout: String::new(),
            stderr: String::new(),
        });
        Box::pin(async move { out })
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn list_dir(&self, _path: &Path) -> Vec<String> {
        Vec::new()
    }

    fn env_var(&self, name: &str) -> Option<String> {
        (name == "PATH").then(|| "SYSTEM_PATH".into())
    }
}

fn report(items: &[(&str, CheckStatus)]) -> DoctorReport {
    DoctorReport {
        items: items
            .iter()
            .map(|(id, status)| CheckItem {
                id: (*id).into(),
                scope: if id.starts_with("msvc") {
                    CheckScope::System
                } else {
                    CheckScope::Isolated
                },
                status: *status,
                found: None,
                wanted: None,
                detail: None,
            })
            .collect(),
    }
}

#[test]
fn plan_installs_only_what_is_not_ok() {
    use CheckStatus::*;
    let r = report(&[
        ("rust", Ok),
        ("wasm32-target", Missing),
        ("trunk", Ok),
        ("wasm-bindgen", WrongVersion),
        ("tauri-cli", Error),
        ("tailwind", Ok),
        ("msvc-build-tools", Missing),
    ]);
    assert_eq!(
        plan(&r),
        vec![
            Step::WasmTarget,
            Step::Tool("wasm-bindgen".into()),
            Step::Tool("tauri-cli".into()),
        ],
        "system items are never installed"
    );
    // Installing Rust brings the target with it.
    let r = report(&[("rust", Missing), ("wasm32-target", Missing)]);
    assert_eq!(plan(&r), vec![Step::Rust]);
}

#[test]
fn rustup_init_never_touches_path() {
    let args = rustup_init_args(&pinned());
    assert!(args.contains(&"--no-modify-path".to_owned()));
    assert!(args.contains(&"-y".to_owned()));
    assert!(
        args.windows(2)
            .any(|w| w == ["--default-toolchain", "1.99.1"])
    );
    assert!(
        args.windows(2)
            .any(|w| w == ["--target", "wasm32-unknown-unknown"])
    );
}

#[tokio::test]
async fn download_verifies_and_resumes() {
    let data: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
    let url = "https://example.test/tool.zip";
    let fetch = FakeFetch {
        files: HashMap::from([(url.to_owned(), data.clone())]),
        ..Default::default()
    };
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("dl").join("tool.zip");
    let part = dir.path().join("dl").join("tool.zip.part");

    // A previous try stopped after 600 bytes.
    std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
    std::fs::write(&part, &data[..600]).unwrap();
    let mut phases = Vec::new();
    download(&fetch, url, &sha(&data), &dest, &mut |p| phases.push(p))
        .await
        .unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), data);
    assert!(!part.exists());
    assert_eq!(fetch.calls.lock().unwrap()[0], (url.to_owned(), 600));
    assert_eq!(
        phases.first(),
        Some(&InstallPhase::Download {
            done: 600,
            size: Some(1000)
        })
    );
    assert_eq!(phases.last(), Some(&InstallPhase::Verify));

    // A server that ignores Range: the partial file is replaced, not appended to.
    let fetch = FakeFetch {
        ignore_range: true,
        ..fetch
    };
    let dest2 = dir.path().join("dl").join("again.zip");
    std::fs::write(dir.path().join("dl").join("again.zip.part"), b"junk").unwrap();
    download(&fetch, url, &sha(&data), &dest2, &mut |_| {})
        .await
        .unwrap();
    assert_eq!(std::fs::read(&dest2).unwrap(), data);
}

#[tokio::test]
async fn bad_checksum_leaves_nothing_behind() {
    let url = "https://example.test/evil";
    let fetch = FakeFetch {
        files: HashMap::from([(url.to_owned(), b"tampered".to_vec())]),
        ..Default::default()
    };
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("evil");
    let err = download(&fetch, url, &sha(b"original"), &dest, &mut |_| {})
        .await
        .unwrap_err();
    assert!(err.contains("checksum mismatch"), "{err}");
    assert!(!dest.exists());
    assert!(
        !dir.path().join("evil.part").exists(),
        "no resuming a bad file"
    );
}

fn zip_with(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in files {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn tar_gz_with(files: &[(&str, &[u8])]) -> Vec<u8> {
    let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut tar = tar::Builder::new(gz);
    for (name, data) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(&mut header, name, *data).unwrap();
    }
    tar.into_inner().unwrap().finish().unwrap()
}

#[test]
fn unpack_takes_the_program_out_of_each_archive_kind() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    let cases: Vec<(Archive, Vec<u8>)> = vec![
        (
            Archive::Zip,
            zip_with(&[("README.md", b"docs"), ("trunk.exe", b"ZIP-PROGRAM")]),
        ),
        (
            Archive::TarGz,
            tar_gz_with(&[
                ("wasm-bindgen-0.2.129/README.md", b"docs"),
                ("wasm-bindgen-0.2.129/trunk.exe", b"TAR-PROGRAM"),
            ]),
        ),
        (Archive::Raw, b"RAW-PROGRAM".to_vec()),
    ];
    for (archive, bytes) in cases {
        let file = dir.path().join("download");
        std::fs::write(&file, &bytes).unwrap();
        let out = unpack(archive, &file, "trunk.exe", &bin).unwrap();
        assert_eq!(out, bin.join("trunk.exe"));
        let got = std::fs::read(&out).unwrap();
        assert!(got.ends_with(b"-PROGRAM"), "{archive:?}");
        assert!(!bin.join("trunk.exe.tmp").exists());
    }

    // The program is missing: no half-written file, the old one stays.
    let file = dir.path().join("download");
    std::fs::write(&file, zip_with(&[("other.exe", b"x")])).unwrap();
    let err = unpack(Archive::Zip, &file, "trunk.exe", &bin).unwrap_err();
    assert!(err.contains("not in the archive"), "{err}");
    assert!(!bin.join("trunk.exe.tmp").exists());
    assert_eq!(
        std::fs::read(bin.join("trunk.exe")).unwrap(),
        b"RAW-PROGRAM"
    );
}

#[tokio::test]
async fn install_runs_rustup_init_privately_and_places_prebuilt_tools() {
    let dir = tempfile::tempdir().unwrap();
    let mut pinned = pinned();
    let rustup_init = assets::rustup_init(&pinned, WIN);
    let tailwind = assets::prebuilt("tailwind", &pinned, WIN).unwrap();
    let trunk = assets::prebuilt("trunk", &pinned, WIN).unwrap();
    let trunk_zip = zip_with(&[("trunk.exe", b"TRUNK")]);
    pinned
        .downloads
        .insert(rustup_init.key.clone(), sha(b"RUSTUP-INIT"));
    pinned
        .downloads
        .insert(tailwind.key.clone(), sha(b"TAILWIND"));
    pinned.downloads.insert(trunk.key.clone(), sha(&trunk_zip));
    // wasm-bindgen and tauri-cli: no pinned checksum, so they fall back to cargo.
    let fetch = FakeFetch {
        files: HashMap::from([
            (rustup_init.url.clone(), b"RUSTUP-INIT".to_vec()),
            (tailwind.url.clone(), b"TAILWIND".to_vec()),
            (trunk.url.clone(), trunk_zip),
        ]),
        ..Default::default()
    };
    let probe = FakeProbe::default();
    let runner = FakeProbe {
        succeed: vec!["rustup-init.exe".into()],
        ..Default::default()
    };
    let tc = Toolchain::new(dir.path().join("toolchain"), &pinned);
    let installer = Installer {
        toolchain: &tc,
        pinned: &pinned,
        platform: Platform::Windows,
        probe: &probe,
        runner: &runner,
        fetch: &fetch,
        target: Some(WIN),
    };
    let progress = Mutex::new(Vec::new());
    let report = installer
        .install(&|p| progress.lock().unwrap().push(p))
        .await;

    // rustup-init: private homes, PATH left alone, no RUSTUP_TOOLCHAIN.
    let calls = runner.calls.lock().unwrap();
    let (program, args, env) = &calls[0];
    assert!(program.ends_with("rustup-init.exe"), "{program}");
    assert_eq!(args, &rustup_init_args(&pinned));
    let env: HashMap<_, _> = env.iter().cloned().collect();
    assert_eq!(
        env["RUSTUP_HOME"],
        tc.rustup_home().to_string_lossy().as_ref()
    );
    assert_eq!(
        env["CARGO_HOME"],
        tc.cargo_home().to_string_lossy().as_ref()
    );
    assert!(!env.contains_key("RUSTUP_TOOLCHAIN"));
    assert!(!env.contains_key("PATH"));

    // Prebuilt tools are in place; downloads are cleaned up.
    assert_eq!(
        std::fs::read(tc.bin().join("tailwindcss.exe")).unwrap(),
        b"TAILWIND"
    );
    assert_eq!(
        std::fs::read(tc.cargo_bin().join("trunk.exe")).unwrap(),
        b"TRUNK"
    );
    let downloads = tc.root().join("downloads");
    assert_eq!(std::fs::read_dir(&downloads).unwrap().count(), 0);

    // wasm-bindgen falls back to `cargo install` from the toolchain, PATH first.
    let (cargo, args, env) = calls
        .iter()
        .find(|(_, a, _)| a.contains(&"wasm-bindgen-cli".to_owned()))
        .unwrap();
    assert_eq!(Path::new(cargo), tc.exe(&tc.cargo_bin(), "cargo"));
    assert!(args.windows(2).any(|w| w == ["--version", "0.2.129"]));
    assert!(args.contains(&"--locked".to_owned()));
    let env: HashMap<_, _> = env.iter().cloned().collect();
    assert_eq!(env["RUSTUP_AUTO_INSTALL"], "0");
    assert!(env["PATH"].ends_with("SYSTEM_PATH"));
    assert!(env["PATH"].starts_with(tc.cargo_bin().to_string_lossy().as_ref()));

    // The fake `cargo` cannot start, so that failure is reported on the item.
    let wb = report.item("wasm-bindgen").unwrap();
    assert_ne!(wb.status, CheckStatus::Ok);
    assert!(wb.detail.as_deref().unwrap().contains("could not start"));

    let progress = progress.lock().unwrap();
    let steps: Vec<(&str, u32, u32)> = progress
        .iter()
        .filter(|p| matches!(p.phase, InstallPhase::Done | InstallPhase::Failed))
        .map(|p| (p.item.as_str(), p.step, p.total))
        .collect();
    assert_eq!(
        steps,
        vec![
            ("rust", 1, 5),
            ("trunk", 2, 5),
            ("wasm-bindgen", 3, 5),
            ("tauri-cli", 4, 5),
            ("tailwind", 5, 5),
        ]
    );
    assert!(
        progress
            .iter()
            .any(|p| p.item == "tauri-cli" && p.phase == InstallPhase::Compile)
    );
}

#[tokio::test]
async fn nothing_to_do_means_no_downloads() {
    // Every isolated item already Ok (macOS, so no Windows system probes either):
    // only the checks run.
    let dir = tempfile::tempdir().unwrap();
    let pinned = pinned();
    let tc = Toolchain::new(dir.path().join("toolchain"), &pinned);
    let fetch = FakeFetch::default();
    let probe = AllOk::new(&tc);
    let installer = Installer {
        toolchain: &tc,
        pinned: &pinned,
        platform: Platform::MacOs,
        probe: &probe,
        runner: &probe,
        fetch: &fetch,
        target: Some("aarch64-apple-darwin"),
    };
    let report = installer.install(&|_| panic!("nothing to install")).await;
    assert!(report.ready(), "{report:?}");
    assert!(fetch.calls.lock().unwrap().is_empty());
}

/// Every pinned tool answers with the pinned version.
struct AllOk {
    outputs: HashMap<String, String>,
}

impl AllOk {
    fn new(tc: &Toolchain) -> Self {
        let cb = tc.cargo_bin();
        let key = |p: PathBuf, args: &str| format!("{} {args}", p.to_string_lossy());
        let outputs = HashMap::from([
            (
                key(tc.exe(&cb, "rustc"), "--version"),
                "rustc 1.99.1".into(),
            ),
            (
                key(tc.exe(&cb, "rustup"), "target list --installed"),
                "wasm32-unknown-unknown".into(),
            ),
            (
                key(tc.exe(&cb, "trunk"), "--version"),
                "trunk 0.21.14".into(),
            ),
            (
                key(tc.exe(&cb, "wasm-bindgen"), "--version"),
                "wasm-bindgen 0.2.129".into(),
            ),
            (
                key(tc.exe(&cb, "cargo-tauri"), "--version"),
                "tauri-cli 2.12.3".into(),
            ),
            (
                key(tc.exe(&tc.bin(), "tailwindcss"), "--help"),
                "tailwindcss v4.3.3".into(),
            ),
            ("xcode-select -p".into(), "/Library/Developer".into()),
        ]);
        Self { outputs }
    }
}

impl Probe for AllOk {
    fn run<'a>(
        &'a self,
        program: &'a str,
        args: &'a [&'a str],
        _env: &'a [(String, String)],
    ) -> BoxFuture<'a, Option<ProbeOutput>> {
        let out = self
            .outputs
            .get(&format!("{program} {}", args.join(" ")))
            .map(|s| ProbeOutput {
                success: true,
                stdout: s.clone(),
                stderr: String::new(),
            });
        Box::pin(async move { out })
    }

    fn exists(&self, _path: &Path) -> bool {
        true
    }

    fn list_dir(&self, _path: &Path) -> Vec<String> {
        Vec::new()
    }

    fn env_var(&self, _name: &str) -> Option<String> {
        None
    }
}
