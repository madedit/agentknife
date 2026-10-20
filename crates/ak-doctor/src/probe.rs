//! How the doctor looks at the computer: run a program, check a path, read an
//! environment variable. Tests replace it with canned answers for any OS.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use futures_util::future::BoxFuture;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

pub trait Probe: Send + Sync {
    /// Runs `program` (a full path or a name on PATH). `None` if it cannot start
    /// (not installed) or does not finish in time.
    fn run<'a>(
        &'a self,
        program: &'a str,
        args: &'a [&'a str],
        env: &'a [(String, String)],
    ) -> BoxFuture<'a, Option<ProbeOutput>>;

    fn exists(&self, path: &Path) -> bool;

    /// Names of the entries in a folder (empty if it does not exist).
    fn list_dir(&self, path: &Path) -> Vec<String>;

    fn env_var(&self, name: &str) -> Option<String>;
}

/// The real computer.
pub struct SystemProbe {
    pub timeout: Duration,
}

impl Default for SystemProbe {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(20),
        }
    }
}

impl Probe for SystemProbe {
    fn run<'a>(
        &'a self,
        program: &'a str,
        args: &'a [&'a str],
        env: &'a [(String, String)],
    ) -> BoxFuture<'a, Option<ProbeOutput>> {
        Box::pin(async move {
            let mut cmd = tokio::process::Command::new(program);
            cmd.args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true);
            for (k, v) in env {
                cmd.env(k, v);
            }
            #[cfg(windows)]
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
            let out = tokio::time::timeout(self.timeout, cmd.output())
                .await
                .ok()?
                .ok()?;
            Some(ProbeOutput {
                success: out.status.success(),
                stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            })
        })
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn list_dir(&self, path: &Path) -> Vec<String> {
        std::fs::read_dir(path)
            .map(|rd| {
                rd.filter_map(Result::ok)
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn env_var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
}
