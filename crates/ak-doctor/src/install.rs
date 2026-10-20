//! Installs the isolated toolchain: only the items `check` did not find Ok,
//! only under the toolchain folder. PATH, dotfiles and the registry are left
//! alone (rustup-init runs with `--no-modify-path`).

use std::io::Read;
use std::path::{Path, PathBuf};

use ak_types::doctor::{CheckScope, CheckStatus, DoctorProgress, DoctorReport, InstallPhase};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::assets::{self, Archive, Asset, Dest};
use crate::detect::{Platform, check};
use crate::fetch::Fetch;
use crate::probe::Probe;
use crate::toolchain::{Pinned, Toolchain};

const WASM_TARGET: &str = "wasm32-unknown-unknown";

/// Report download progress about every this many bytes.
const PROGRESS_EVERY: u64 = 512 * 1024;

/// One thing to install, named by its doctor item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Rust itself, with the wasm32 target (rustup-init, or rustup if present).
    Rust,
    /// Only the wasm32 target is missing.
    WasmTarget,
    /// A prebuilt tool (`trunk`, `wasm-bindgen`, `tauri-cli`, `tailwind`).
    Tool(String),
}

impl Step {
    /// The doctor item id.
    pub fn item(&self) -> &str {
        match self {
            Step::Rust => "rust",
            Step::WasmTarget => "wasm32-target",
            Step::Tool(id) => id,
        }
    }
}

/// What to install for `report`, in order. System items are never installed:
/// the user installs those with AgentKnife's guidance.
pub fn plan(report: &DoctorReport) -> Vec<Step> {
    let needs = |id: &str| {
        report
            .item(id)
            .is_some_and(|i| i.scope == CheckScope::Isolated && i.status != CheckStatus::Ok)
    };
    let mut steps = Vec::new();
    if needs("rust") {
        // Installing Rust adds the target too.
        steps.push(Step::Rust);
    } else if needs("wasm32-target") {
        steps.push(Step::WasmTarget);
    }
    for tool in ["trunk", "wasm-bindgen", "tauri-cli", "tailwind"] {
        if needs(tool) {
            steps.push(Step::Tool(tool.into()));
        }
    }
    steps
}

/// The crate `cargo install` builds when a tool has no prebuilt download.
fn fallback_crate<'p>(tool: &str, pinned: &'p Pinned) -> Option<(&'static str, &'p str)> {
    match tool {
        "trunk" => Some(("trunk", &pinned.trunk)),
        "wasm-bindgen" => Some(("wasm-bindgen-cli", &pinned.wasm_bindgen)),
        "tauri-cli" => Some(("tauri-cli", &pinned.tauri_cli)),
        _ => None,
    }
}

/// Arguments for rustup-init. `--no-modify-path` keeps the user's PATH,
/// dotfiles and registry untouched.
pub fn rustup_init_args(pinned: &Pinned) -> Vec<String> {
    [
        "-y",
        "--no-modify-path",
        "--profile",
        "minimal",
        "--default-toolchain",
        &pinned.rust,
        "--target",
        WASM_TARGET,
    ]
    .map(str::to_owned)
    .to_vec()
}

pub struct Installer<'a> {
    pub toolchain: &'a Toolchain,
    pub pinned: &'a Pinned,
    pub platform: Platform,
    /// For the checks before and after (short timeout).
    pub probe: &'a dyn Probe,
    /// For installers and `cargo install` (long timeout).
    pub runner: &'a dyn Probe,
    pub fetch: &'a dyn Fetch,
    /// The host's prebuilt target (`assets::host_target()`); `None` compiles everything.
    pub target: Option<&'a str>,
}

impl Installer<'_> {
    /// Checks, installs what is missing, and checks again. Install errors end up
    /// in the `detail` of the items that are still not Ok.
    pub async fn install(&self, progress: &(dyn Fn(DoctorProgress) + Send + Sync)) -> DoctorReport {
        let before = check(self.toolchain, self.pinned, self.platform, self.probe).await;
        let steps = plan(&before);
        let total = u32::try_from(steps.len()).unwrap_or(u32::MAX);
        let mut errors = Vec::new();
        for (n, step) in (1..).zip(&steps) {
            let emit = |phase| {
                progress(DoctorProgress {
                    item: step.item().to_owned(),
                    step: n,
                    total,
                    phase,
                })
            };
            let result = match step {
                Step::Rust => self.rust(&emit).await,
                Step::WasmTarget => self.wasm_target(&emit).await,
                Step::Tool(tool) => self.tool(tool, &emit).await,
            };
            match result {
                Ok(()) => emit(InstallPhase::Done),
                Err(e) => {
                    emit(InstallPhase::Failed);
                    errors.push((step.item().to_owned(), e));
                }
            }
        }
        let mut after = check(self.toolchain, self.pinned, self.platform, self.probe).await;
        for (id, error) in errors {
            if let Some(item) = after.items.iter_mut().find(|i| i.id == id)
                && item.status != CheckStatus::Ok
            {
                item.detail = Some(error);
            }
        }
        after
    }

    fn downloads(&self) -> PathBuf {
        self.toolchain.root().join("downloads")
    }

    /// rustup-init's environment: the private homes only. No `RUSTUP_TOOLCHAIN`
    /// (it would name a toolchain that does not exist yet).
    fn init_env(&self) -> Vec<(String, String)> {
        let tc = self.toolchain;
        vec![
            (
                "RUSTUP_HOME".into(),
                tc.rustup_home().to_string_lossy().into_owned(),
            ),
            (
                "CARGO_HOME".into(),
                tc.cargo_home().to_string_lossy().into_owned(),
            ),
            ("RUSTUP_AUTO_INSTALL".into(), "0".into()),
            // The user's own Rust on PATH is fine: ours is separate.
            ("RUSTUP_INIT_SKIP_PATH_CHECK".into(), "yes".into()),
        ]
    }

    /// `Toolchain::env()` plus the toolchain first on PATH (for this process only).
    fn tool_env(&self) -> Vec<(String, String)> {
        let mut env = self.toolchain.env();
        let mut path: Vec<PathBuf> = self.toolchain.path_prefix();
        if let Some(system) = self.probe.env_var("PATH") {
            path.extend(std::env::split_paths(&system));
        }
        if let Ok(joined) = std::env::join_paths(path) {
            env.push(("PATH".into(), joined.to_string_lossy().into_owned()));
        }
        env
    }

    async fn run(
        &self,
        program: &Path,
        args: &[String],
        env: &[(String, String)],
    ) -> Result<(), String> {
        let program = program.to_string_lossy();
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        match self.runner.run(&program, &args, env).await {
            Some(out) if out.success => Ok(()),
            Some(out) => {
                let text = format!("{}\n{}", out.stdout, out.stderr);
                let text = text.trim();
                // The end of the output is where the error is.
                let start = text.len().saturating_sub(2000);
                let start = (start..text.len())
                    .find(|&i| text.is_char_boundary(i))
                    .unwrap_or(0);
                Err(format!("{program} failed:\n{}", &text[start..]))
            }
            None => Err(format!("{program} could not start or did not finish")),
        }
    }

    async fn rust(&self, emit: &(dyn Fn(InstallPhase) + Sync)) -> Result<(), String> {
        let tc = self.toolchain;
        let rustup = tc.exe(&tc.cargo_bin(), "rustup");
        if self.probe.exists(&rustup) {
            // rustup is ours already: add the pinned toolchain.
            emit(InstallPhase::Setup);
            let args = [
                "toolchain",
                "install",
                &self.pinned.rust,
                "--profile",
                "minimal",
                "--target",
                WASM_TARGET,
            ]
            .map(str::to_owned);
            return self.run(&rustup, &args, &self.init_env()).await;
        }
        let target = self
            .target
            .ok_or("no pinned rustup-init download for this computer")?;
        let asset = assets::rustup_init(self.pinned, target);
        let file = self.fetch_verified(&asset, emit).await?;
        make_executable(&file)?;
        emit(InstallPhase::Setup);
        let result = self
            .run(&file, &rustup_init_args(self.pinned), &self.init_env())
            .await;
        let _ = std::fs::remove_file(&file);
        result
    }

    async fn wasm_target(&self, emit: &(dyn Fn(InstallPhase) + Sync)) -> Result<(), String> {
        let tc = self.toolchain;
        emit(InstallPhase::Setup);
        let args = [
            "target",
            "add",
            WASM_TARGET,
            "--toolchain",
            &self.pinned.rust,
        ]
        .map(str::to_owned);
        self.run(&tc.exe(&tc.cargo_bin(), "rustup"), &args, &tc.env())
            .await
    }

    async fn tool(&self, tool: &str, emit: &(dyn Fn(InstallPhase) + Sync)) -> Result<(), String> {
        let prebuilt = match self
            .target
            .and_then(|t| assets::prebuilt(tool, self.pinned, t))
        {
            Some(asset) => match self.install_prebuilt(&asset, emit).await {
                Ok(()) => return Ok(()),
                Err(e) => Some(e),
            },
            None => None,
        };
        let Some((krate, version)) = fallback_crate(tool, self.pinned) else {
            return Err(
                prebuilt.unwrap_or_else(|| format!("no download of {tool} for this computer"))
            );
        };
        emit(InstallPhase::Compile);
        let tc = self.toolchain;
        let args = [
            "install",
            "--locked",
            "--force",
            "--root",
            &tc.cargo_home().to_string_lossy(),
            krate,
            "--version",
            version,
        ]
        .map(|s| s.to_owned());
        self.run(&tc.exe(&tc.cargo_bin(), "cargo"), &args, &self.tool_env())
            .await
            .map_err(|e| match prebuilt {
                Some(p) => format!("{p}\n\nthen cargo install: {e}"),
                None => e,
            })
    }

    async fn install_prebuilt(
        &self,
        asset: &Asset,
        emit: &(dyn Fn(InstallPhase) + Sync),
    ) -> Result<(), String> {
        let file = self.fetch_verified(asset, emit).await?;
        emit(InstallPhase::Unpack);
        let dir = match asset.dest {
            Dest::CargoBin => self.toolchain.cargo_bin(),
            Dest::Bin => self.toolchain.bin(),
        };
        let (archive, binary, from) = (asset.archive, asset.binary.clone(), file.clone());
        tokio::task::spawn_blocking(move || unpack(archive, &from, &binary, &dir))
            .await
            .map_err(|e| e.to_string())??;
        let _ = std::fs::remove_file(&file);
        Ok(())
    }

    async fn fetch_verified(
        &self,
        asset: &Asset,
        emit: &(dyn Fn(InstallPhase) + Sync),
    ) -> Result<PathBuf, String> {
        let sha256 = self
            .pinned
            .checksum(&asset.key)
            .ok_or_else(|| format!("no pinned checksum for {}", asset.key))?;
        let dest = self.downloads().join(&asset.file_name);
        download(self.fetch, &asset.url, sha256, &dest, &mut |p| emit(p)).await?;
        Ok(dest)
    }
}

/// Downloads `url` to `dest`, resuming `dest.part` if a previous try stopped,
/// and checks the sha256 before `dest` appears. Reports `Download` and `Verify`.
pub async fn download(
    fetch: &dyn Fetch,
    url: &str,
    sha256: &str,
    dest: &Path,
    on_phase: &mut (dyn FnMut(InstallPhase) + Send),
) -> Result<(), String> {
    let io = |e: std::io::Error| format!("{}: {e}", dest.display());
    if let Some(dir) = dest.parent() {
        tokio::fs::create_dir_all(dir).await.map_err(io)?;
    }
    let part = part_path(dest);
    let from = tokio::fs::metadata(&part)
        .await
        .map(|m| m.len())
        .unwrap_or(0);
    let mut dl = fetch.get(url, from).await?;
    let mut done = if dl.resumed { from } else { 0 };
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(dl.resumed)
        .truncate(!dl.resumed)
        .open(&part)
        .await
        .map_err(io)?;
    on_phase(InstallPhase::Download {
        done,
        size: dl.size,
    });
    let mut reported = done;
    while let Some(chunk) = dl.body.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await.map_err(io)?;
        done += chunk.len() as u64;
        if done - reported >= PROGRESS_EVERY {
            reported = done;
            on_phase(InstallPhase::Download {
                done,
                size: dl.size,
            });
        }
    }
    file.flush().await.map_err(io)?;
    drop(file);
    on_phase(InstallPhase::Download {
        done,
        size: dl.size,
    });

    on_phase(InstallPhase::Verify);
    let hashed = part.clone();
    let got = tokio::task::spawn_blocking(move || sha256_file(&hashed))
        .await
        .map_err(|e| e.to_string())?
        .map_err(io)?;
    if !got.eq_ignore_ascii_case(sha256) {
        // A bad file cannot be resumed into a good one.
        let _ = tokio::fs::remove_file(&part).await;
        return Err(format!(
            "{url}: checksum mismatch (expected {sha256}, got {got})"
        ));
    }
    tokio::fs::rename(&part, dest).await.map_err(io)
}

fn part_path(dest: &Path) -> PathBuf {
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    dest.with_file_name(name)
}

fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Puts the program `binary` from `file` into `dir`, through a temporary file
/// and a rename, so an interrupted install never leaves half a program.
pub fn unpack(archive: Archive, file: &Path, binary: &str, dir: &Path) -> Result<PathBuf, String> {
    let err = |e: &dyn std::fmt::Display| format!("{}: {e}", file.display());
    std::fs::create_dir_all(dir).map_err(|e| err(&e))?;
    let dest = dir.join(binary);
    let tmp = dir.join(format!("{binary}.tmp"));
    let mut out = std::fs::File::create(&tmp).map_err(|e| err(&e))?;
    let copied = match archive {
        Archive::Raw => {
            let mut src = std::fs::File::open(file).map_err(|e| err(&e))?;
            std::io::copy(&mut src, &mut out).map(|_| true)
        }
        Archive::Zip => {
            let src = std::fs::File::open(file).map_err(|e| err(&e))?;
            let mut zip = zip::ZipArchive::new(src).map_err(|e| err(&e))?;
            let mut found = false;
            for i in 0..zip.len() {
                let mut entry = zip.by_index(i).map_err(|e| err(&e))?;
                let is_binary = entry.is_file()
                    && entry
                        .name()
                        .is_ok_and(|n| n.rsplit(['/', '\\']).next() == Some(binary));
                if is_binary {
                    std::io::copy(&mut entry, &mut out).map_err(|e| err(&e))?;
                    found = true;
                    break;
                }
            }
            Ok(found)
        }
        Archive::TarGz => {
            let src = std::fs::File::open(file).map_err(|e| err(&e))?;
            let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(src));
            let mut found = false;
            for entry in tar.entries().map_err(|e| err(&e))? {
                let mut entry = entry.map_err(|e| err(&e))?;
                let is_binary = entry.header().entry_type().is_file()
                    && entry
                        .path()
                        .ok()
                        .and_then(|p| p.file_name().map(|n| n == binary))
                        .unwrap_or(false);
                if is_binary {
                    std::io::copy(&mut entry, &mut out).map_err(|e| err(&e))?;
                    found = true;
                    break;
                }
            }
            Ok(found)
        }
    };
    let done = copied
        .map_err(|e| err(&e))
        .and_then(|found| {
            found
                .then_some(())
                .ok_or_else(|| err(&format!("{binary} is not in the archive")))
        })
        .and_then(|()| out.sync_all().map_err(|e| err(&e)));
    drop(out);
    if let Err(e) = done {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    make_executable(&tmp)?;
    std::fs::rename(&tmp, &dest).map_err(|e| err(&e))?;
    Ok(dest)
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<(), String> {
    Ok(())
}
