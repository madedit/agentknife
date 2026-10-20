//! Host-platform bundling: builds the app in release mode and leaves the user one
//! file to install from (a stack's `BundlePlan` says how); [`guide`] says how to
//! install it.

#![forbid(unsafe_code)]

pub mod guide;

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ak_preview::Tools;
use ak_preview::log::Tail;
use ak_preview::process::{kill_tree, read_lines};
use ak_project::{CheckpointKind, ProjectError, Timeline};
use ak_types::stack::{
    ArchiveFormat, BundleOutput, BundlePlan, HostOs, JsonField, PreviewMarker, ScanIn,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// What the user sees while bundling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BundleEvent {
    Stage(BundleStage),
    /// A line of build output (colors removed).
    Output(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundleStage {
    /// The release build (minutes the first time).
    Building,
    /// Making sure no preview-only code is in the build.
    Checking,
    /// Packing the program into an archive.
    Packing,
}

impl BundleStage {
    /// Fluent message id of the progress label.
    pub fn message_id(self) -> &'static str {
        match self {
            BundleStage::Building => "bundle-building",
            BundleStage::Checking => "bundle-checking",
            BundleStage::Packing => "bundle-packing",
        }
    }
}

/// A finished bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    /// The file to hand to the user.
    pub path: PathBuf,
    pub output: BundleOutput,
    /// The app's folder and program name inside it, as the user will see it.
    pub app: String,
    /// Id of the `Bundle` timeline entry for the source it was built from.
    pub checkpoint: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum BundleError {
    #[error("this stack cannot be bundled on this system")]
    UnsupportedHost,
    #[error("cannot read the app's {what} ({pointer} in {file})")]
    AppInfo {
        what: &'static str,
        file: &'static str,
        pointer: &'static str,
    },
    #[error("the release build failed")]
    BuildFailed { output: String },
    #[error("the build did not produce {0}")]
    Missing(PathBuf),
    #[error("the release build contains preview-only code (`{marker}` in {file})")]
    PreviewCode { marker: &'static str, file: PathBuf },
    #[error("bundling was cancelled")]
    Cancelled,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Project(#[from] ProjectError),
}

pub struct Bundler {
    root: PathBuf,
    plan: BundlePlan,
    tools: Tools,
    os: Option<HostOs>,
}

impl Bundler {
    /// Bundles for this computer's OS.
    pub fn new(root: &Path, plan: BundlePlan, tools: Tools) -> Bundler {
        Bundler {
            root: root.to_owned(),
            plan,
            tools,
            os: HostOs::current(),
        }
    }

    /// Builds, checks, packs and records a `Bundle` checkpoint. Progress goes to
    /// `events`; cancelling stops the build with everything it started.
    pub async fn build(
        &self,
        events: &mpsc::UnboundedSender<BundleEvent>,
        cancel: &CancellationToken,
    ) -> Result<Bundle, BundleError> {
        let target = self
            .os
            .and_then(|os| self.plan.target(os))
            .ok_or(BundleError::UnsupportedHost)?;
        let name = read_field(&self.root, self.plan.name, "name")?;
        let version = read_field(&self.root, self.plan.version, "version")?;
        let started = SystemTime::now();

        let _ = events.send(BundleEvent::Stage(BundleStage::Building));
        self.run(&target.command, events, cancel).await?;

        let _ = events.send(BundleEvent::Stage(BundleStage::Checking));
        let program = self.program();
        if !program.is_file() {
            return Err(BundleError::Missing(program));
        }
        let (root, markers) = (self.root.clone(), self.plan.preview_markers);
        let scanned = program.clone();
        blocking(move || check_markers(&root, &scanned, markers)).await?;

        let output = target.output;
        let path = match output {
            BundleOutput::Archive(format) => {
                let _ = events.send(BundleEvent::Stage(BundleStage::Packing));
                let file = format!(
                    "{}_{version}_{}.{}",
                    file_name(&name),
                    arch(),
                    format.extension()
                );
                let dest = self
                    .root
                    .join("target")
                    .join("release")
                    .join("bundle")
                    .join(format.extension())
                    .join(file);
                let (dest2, folder) = (dest.clone(), file_name(&name));
                blocking(move || pack(format, &program, &folder, &dest2)).await?;
                dest
            }
            BundleOutput::Installer { dir, extension, .. } => {
                newest(&self.root.join(dir), extension, started)?
            }
        };
        if cancel.is_cancelled() {
            return Err(BundleError::Cancelled);
        }

        let label = path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default();
        let root = self.root.clone();
        let checkpoint = blocking(move || -> Result<_, BundleError> {
            let entry = Timeline::open(&root)?.checkpoint(CheckpointKind::Bundle, &label, None)?;
            Ok(entry.map(|e| e.id))
        })
        .await?;
        Ok(Bundle {
            path,
            output,
            app: file_name(&name),
            checkpoint,
        })
    }

    /// The release program (with `.exe` on Windows).
    pub fn program(&self) -> PathBuf {
        let exe = if self.os == Some(HostOs::Windows) {
            ".exe"
        } else {
            ""
        };
        self.root.join(format!("{}{exe}", self.plan.program))
    }

    async fn run(
        &self,
        spec: &ak_types::stack::CommandSpec,
        events: &mpsc::UnboundedSender<BundleEvent>,
        cancel: &CancellationToken,
    ) -> Result<(), BundleError> {
        // stdout and stderr share one pipe, so compiler errors stay in order.
        let (reader, writer) = std::io::pipe()?;
        let mut cmd = self.tools.command(spec, &self.root, &[]);
        cmd.stdout(writer.try_clone()?).stderr(writer);
        let mut child = cmd.spawn()?;
        drop(cmd);
        let pid = child.id();
        let (tx, mut rx) = mpsc::unbounded_channel();
        read_lines(reader, tx);
        let mut tail = Tail::default();
        let mut lines_done = false;
        let status = loop {
            tokio::select! {
                () = cancel.cancelled() => {
                    if let Some(pid) = pid {
                        kill_tree(pid).await;
                    }
                    let _ = child.wait().await;
                    return Err(BundleError::Cancelled);
                }
                line = rx.recv(), if !lines_done => match line {
                    Some(raw) => {
                        let line = tail.push(&raw);
                        let _ = events.send(BundleEvent::Output(line));
                    }
                    None => lines_done = true,
                },
                status = child.wait(), if lines_done => break status?,
            }
        };
        if status.success() {
            Ok(())
        } else {
            Err(BundleError::BuildFailed {
                output: tail.text(),
            })
        }
    }
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, BundleError> + Send + 'static,
) -> Result<T, BundleError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| BundleError::Io(std::io::Error::other(e)))?
}

fn read_field(root: &Path, field: JsonField, what: &'static str) -> Result<String, BundleError> {
    let err = || BundleError::AppInfo {
        what,
        file: field.file,
        pointer: field.pointer,
    };
    let text = std::fs::read_to_string(root.join(field.file)).map_err(|_| err())?;
    let json: serde_json::Value = serde_json::from_str(&text).map_err(|_| err())?;
    json.pointer(field.pointer)
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .ok_or_else(err)
}

/// The app name as a file and folder name on every OS (`My App`, `待辦清單`).
pub fn file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| !c.is_control() && !r#"<>:"/\|?*"#.contains(*c))
        .collect();
    let cleaned = cleaned.trim_matches(|c: char| c == '.' || c.is_whitespace());
    if cleaned.is_empty() {
        "app".to_owned()
    } else {
        cleaned.to_owned()
    }
}

/// The CPU in archive names, as users and Tauri name it.
fn arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "x86" => "x86",
        other => other,
    }
}

fn check_markers(
    root: &Path,
    program: &Path,
    markers: &'static [PreviewMarker],
) -> Result<(), BundleError> {
    for m in markers {
        let files = match m.scan {
            ScanIn::Program => vec![program.to_owned()],
            ScanIn::Dir(dir) => files_under(&root.join(dir))?,
        };
        for file in files {
            let bytes = std::fs::read(&file)?;
            if contains(&bytes, m.text.as_bytes()) {
                return Err(BundleError::PreviewCode {
                    marker: m.text,
                    file,
                });
            }
        }
    }
    Ok(())
}

fn files_under(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(out);
    };
    for entry in entries {
        let path = entry?.path();
        if path.is_dir() {
            out.extend(files_under(&path)?);
        } else {
            out.push(path);
        }
    }
    Ok(out)
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle)
}

/// Packs `program` as `<folder>/<folder>[.exe]` into a new archive at `dest`.
fn pack(
    format: ArchiveFormat,
    program: &Path,
    folder: &str,
    dest: &Path,
) -> Result<(), BundleError> {
    let dir = dest
        .parent()
        .ok_or_else(|| BundleError::Missing(dest.to_owned()))?;
    std::fs::create_dir_all(dir)?;
    let ext = program
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let inner = format!("{folder}/{folder}{ext}");
    let mut data = Vec::new();
    std::fs::File::open(program)?.read_to_end(&mut data)?;
    // Written next to the destination, then moved: never a half archive.
    let tmp = dest.with_extension("partial");
    let file = std::fs::File::create(&tmp)?;
    match format {
        ArchiveFormat::Zip => {
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .unix_permissions(0o755)
                .large_file(data.len() > u32::MAX as usize);
            zip.start_file(inner, options).map_err(zip_err)?;
            zip.write_all(&data)?;
            zip.finish().map_err(zip_err)?.sync_all()?;
        }
        ArchiveFormat::TarGz => {
            let gz = flate2::write::GzEncoder::new(file, flate2::Compression::default());
            let mut tar = tar::Builder::new(gz);
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o755);
            header.set_mtime(
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            );
            tar.append_data(&mut header, &inner, data.as_slice())?;
            tar.into_inner()?.finish()?.sync_all()?;
        }
    }
    std::fs::rename(&tmp, dest)?;
    Ok(())
}

fn zip_err(e: zip::result::ZipError) -> BundleError {
    BundleError::Io(std::io::Error::other(e))
}

/// The newest `*.{extension}` in `dir` written since `since`.
fn newest(dir: &Path, extension: &str, since: SystemTime) -> Result<PathBuf, BundleError> {
    let missing = || BundleError::Missing(dir.join(format!("*.{extension}")));
    let mut best: Option<(SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(dir).map_err(|_| missing())? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_none_or(|e| e != extension) {
            continue;
        }
        let modified = entry.metadata()?.modified()?;
        if modified >= since && best.as_ref().is_none_or(|(t, _)| modified > *t) {
            best = Some((modified, path));
        }
    }
    best.map(|(_, p)| p).ok_or_else(missing)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_work_on_every_os() {
        assert_eq!(file_name("My App"), "My App");
        assert_eq!(file_name("待辦清單"), "待辦清單");
        assert_eq!(file_name(" a/b:c? "), "abc");
        assert_eq!(file_name("..."), "app");
        assert_eq!(file_name(""), "app");
    }

    #[test]
    fn tar_gz_keeps_the_program_runnable() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let program = dir.path().join("app");
        std::fs::write(&program, b"program")?;
        let dest = dir.path().join("out/My App_1.0.0_x64.tar.gz");
        pack(ArchiveFormat::TarGz, &program, "My App", &dest)?;
        let file = std::fs::File::open(&dest)?;
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
        let mut entries = tar.entries()?;
        let mut entry = entries.next().ok_or("empty archive")??;
        assert_eq!(entry.path()?.to_string_lossy(), "My App/My App");
        assert_eq!(entry.header().mode()? & 0o777, 0o755);
        let mut data = Vec::new();
        entry.read_to_end(&mut data)?;
        assert_eq!(data, b"program");
        assert!(!dest.with_extension("partial").exists());
        Ok(())
    }

    #[test]
    fn finds_markers() {
        assert!(contains(b"xxAK_BRIDGE_READY yy", b"AK_BRIDGE_READY"));
        assert!(!contains(b"AK_BRIDGE", b"AK_BRIDGE_READY"));
        assert!(!contains(b"abc", b""));
    }
}
