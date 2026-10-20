//! Starting toolchain programs for a project, and stopping them with everything
//! they started (`cargo run` starts the app; trunk starts cargo).

use std::path::{Path, PathBuf};
use std::process::Stdio;

use ak_doctor::Toolchain;
use ak_types::stack::{CommandSpec, Tool, fill};

/// Inherited variables that would make cargo/rustup use another toolchain.
const INHERITED: &[&str] = &[
    "CARGO",
    "RUSTC",
    "RUSTDOC",
    "RUSTC_WRAPPER",
    "RUSTUP_TOOLCHAIN",
    "RUSTUP_HOME",
    "CARGO_HOME",
    "CARGO_TARGET_DIR",
];

/// Where the toolchain's programs are, and the environment to run them in.
#[derive(Debug, Clone)]
pub struct Tools {
    pub cargo: PathBuf,
    pub trunk: PathBuf,
    pub cargo_tauri: PathBuf,
    /// Set for every program (RUSTUP_HOME, CARGO_HOME, …).
    pub env: Vec<(String, String)>,
    /// Put first on PATH.
    pub path_prefix: Vec<PathBuf>,
}

impl Tools {
    /// AgentKnife's isolated toolchain.
    pub fn from_toolchain(tc: &Toolchain) -> Tools {
        let bin = tc.cargo_bin();
        Tools {
            cargo: tc.exe(&bin, "cargo"),
            trunk: tc.exe(&bin, "trunk"),
            cargo_tauri: tc.exe(&bin, "cargo-tauri"),
            env: tc.env(),
            path_prefix: tc.path_prefix(),
        }
    }

    pub fn exe(&self, tool: Tool) -> &Path {
        match tool {
            Tool::Cargo => &self.cargo,
            Tool::Trunk => &self.trunk,
            Tool::CargoTauri => &self.cargo_tauri,
        }
    }

    /// A ready-to-spawn command for `spec` in `root`, with `values` filled into
    /// its `{placeholders}`. The caller sets stdout/stderr.
    pub fn command(
        &self,
        spec: &CommandSpec,
        root: &Path,
        values: &[(&str, &str)],
    ) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new(self.exe(spec.tool));
        cmd.args(spec.args.iter().map(|a| fill(a, values)))
            .current_dir(root.join(spec.cwd))
            .stdin(Stdio::null())
            .kill_on_drop(true);
        for k in INHERITED
            .iter()
            .copied()
            .chain(spec.env_remove.iter().map(String::as_str))
        {
            cmd.env_remove(k);
        }
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        let mut path = self.path_prefix.clone();
        if let Some(system) = std::env::var_os("PATH") {
            path.extend(std::env::split_paths(&system));
        }
        if let Ok(joined) = std::env::join_paths(path) {
            cmd.env("PATH", joined);
        }
        for (k, v) in &spec.env {
            cmd.env(k, fill(v, values));
        }
        // Its own process group, so the whole tree can be stopped.
        #[cfg(unix)]
        cmd.process_group(0);
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        cmd
    }
}

fn killer(pid: u32) -> std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut k = std::process::Command::new("taskkill");
        k.args(["/T", "/F", "/PID", &pid.to_string()]);
        k.creation_flags(0x0800_0000);
        k
    }
    #[cfg(not(windows))]
    {
        // The program leads its own process group (process_group(0)): pgid == pid.
        let mut k = std::process::Command::new("kill");
        k.args(["-KILL", "--", &format!("-{pid}")]);
        k
    }
}

/// Stops a process and everything it started. Best effort.
pub async fn kill_tree(pid: u32) {
    let mut k = tokio::process::Command::from(killer(pid));
    let _ = k
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;
}

/// [`kill_tree`] for `Drop` (no async runtime needed).
pub fn kill_tree_now(pid: u32) {
    let _ = killer(pid)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// A free TCP port on 127.0.0.1 (released again for the program to bind).
pub fn free_port() -> std::io::Result<u16> {
    Ok(std::net::TcpListener::bind(("127.0.0.1", 0))?
        .local_addr()?
        .port())
}

/// 128 random bits, hex: the bridge token.
pub fn random_token() -> std::io::Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Percent-encodes a URL query value.
pub fn encode_query_value(v: &str) -> String {
    v.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_ports_and_encoding() {
        let a = random_token().unwrap();
        assert_eq!(a.len(), 32);
        assert_ne!(a, random_token().unwrap());
        assert!(free_port().unwrap() > 0);
        assert_eq!(
            encode_query_value("ws://127.0.0.1:5/?token=ab"),
            "ws%3A%2F%2F127.0.0.1%3A5%2F%3Ftoken%3Dab"
        );
    }
}
