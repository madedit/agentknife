//! run_command: a shell command in the project folder.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use ak_types::chat::ToolSpec;
use futures_util::future::BoxFuture;
use serde_json::{Value, json};

use super::{MAX_OUTPUT_CHARS, cap, project_root, str_arg};
use crate::tool::{Tool, ToolContext, ToolOutput};

const DEFAULT_TIMEOUT_SECS: u64 = 300;
const MAX_TIMEOUT_SECS: u64 = 1_800;

/// Environment for commands: AgentKnife's isolated toolchain comes first on PATH.
#[derive(Debug, Clone, Default)]
pub struct ToolEnv {
    pub path_prefix: Vec<PathBuf>,
    pub vars: Vec<(String, String)>,
}

pub struct RunCommand {
    env: ToolEnv,
}

impl RunCommand {
    pub fn new(env: ToolEnv) -> Self {
        Self { env }
    }

    fn command(&self, line: &str) -> tokio::process::Command {
        #[cfg(windows)]
        let mut cmd = {
            let mut c = tokio::process::Command::new("cmd");
            // Passed verbatim: Rust's argument quoting escapes `"` as `\"`, which cmd
            // does not understand. With /S, cmd strips only the outer quotes.
            // chcp 65001 makes programs that follow the console code page print UTF-8.
            c.raw_arg(format!("/S /C \"chcp 65001>nul & {line}\""));
            // CREATE_NO_WINDOW: no console window flashes up from the GUI app.
            c.creation_flags(0x0800_0000);
            c
        };
        #[cfg(not(windows))]
        let mut cmd = {
            let mut c = tokio::process::Command::new("sh");
            c.arg("-c").arg(line);
            c
        };
        if !self.env.path_prefix.is_empty() {
            let current = std::env::var_os("PATH").unwrap_or_default();
            let paths = self
                .env
                .path_prefix
                .iter()
                .cloned()
                .chain(std::env::split_paths(&current));
            if let Ok(joined) = std::env::join_paths(paths) {
                cmd.env("PATH", joined);
            }
        }
        for (k, v) in &self.env.vars {
            cmd.env(k, v);
        }
        // Own process group, so the whole tree can be stopped (see `kill_tree`).
        #[cfg(unix)]
        cmd.process_group(0);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        cmd
    }
}

/// Stops a command and everything it started. Best effort: the command may have
/// exited already.
async fn kill_tree(pid: Option<u32>) {
    let Some(pid) = pid else { return };
    #[cfg(windows)]
    let mut killer = {
        let mut k = tokio::process::Command::new("taskkill");
        k.args(["/T", "/F", "/PID", &pid.to_string()]);
        k.creation_flags(0x0800_0000);
        k
    };
    #[cfg(not(windows))]
    let mut killer = {
        // The shell leads its own process group (process_group(0)), so pgid == pid.
        let mut k = tokio::process::Command::new("kill");
        k.args(["-KILL", "--", &format!("-{pid}")]);
        k
    };
    let _ = killer
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;
}

impl Tool for RunCommand {
    fn changes_project(&self) -> bool {
        true
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "run_command".into(),
            description: "Run a shell command in the project folder (Windows: cmd, otherwise sh), \
                          e.g. `cargo build`. Returns the exit code and output. There is no \
                          keyboard: commands that wait for a key or a terminal (`timeout`, \
                          `pause`) fail. To wait for the app in the preview, use `interact` \
                          `read_state` with `wait_ms` instead."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" },
                    "timeout_secs": { "type": "integer", "description": "Default 300, maximum 1800." },
                },
                "required": ["command"],
            }),
        }
    }

    fn call<'a>(&'a self, args: Value, ctx: &'a ToolContext) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            let root = match project_root(ctx) {
                Ok(r) => r.to_path_buf(),
                Err(e) => return e,
            };
            let line = match str_arg(&args, "command") {
                Ok(l) => l.to_owned(),
                Err(e) => return e,
            };
            let secs = args["timeout_secs"]
                .as_u64()
                .map_or(DEFAULT_TIMEOUT_SECS, |s| s.clamp(1, MAX_TIMEOUT_SECS));
            let mut cmd = self.command(&line);
            cmd.current_dir(&root);
            let child = match cmd.spawn() {
                Ok(c) => c,
                Err(e) => return ToolOutput::error(format!("Could not start the command: {e}")),
            };
            let pid = child.id();
            let wait = child.wait_with_output();
            tokio::pin!(wait);
            // Killing only the shell would leave its children (cargo, trunk …)
            // running, so stop the whole process tree.
            let output = tokio::select! {
                o = &mut wait => o,
                _ = tokio::time::sleep(Duration::from_secs(secs)) => {
                    kill_tree(pid).await;
                    return ToolOutput::error(format!("The command was stopped after {secs} seconds."));
                }
                _ = ctx.cancel.cancelled() => {
                    kill_tree(pid).await;
                    return ToolOutput::error("Cancelled by the user.");
                }
            };
            let output = match output {
                Ok(o) => o,
                Err(e) => return ToolOutput::error(format!("The command failed to run: {e}")),
            };
            let code = output
                .status
                .code()
                .map_or_else(|| "none (killed)".to_owned(), |c| c.to_string());
            let mut text = output_text(&output.stdout);
            let stderr = output_text(&output.stderr);
            if !stderr.trim().is_empty() {
                text.push_str("\n[stderr]\n");
                text.push_str(&stderr);
            }
            let body = format!(
                "exit code: {code}\n{}",
                cap(text.trim_end(), MAX_OUTPUT_CHARS)
            );
            if output.status.success() {
                ToolOutput::text(body)
            } else {
                ToolOutput::error(body)
            }
        })
    }
}

/// A command's output as text. UTF-8 lines stay as they are; other lines are
/// cmd's own commands (`cd`, `echo`, `dir`), which write the system's ANSI code
/// page (Big5 on Traditional Chinese Windows) whatever `chcp` says.
fn output_text(bytes: &[u8]) -> String {
    decode_lines(bytes, ansi_encoding())
}

fn decode_lines(bytes: &[u8], legacy: Option<&'static encoding_rs::Encoding>) -> String {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_owned();
    }
    bytes
        .split_inclusive(|b| *b == b'\n')
        .map(|line| match (std::str::from_utf8(line), legacy) {
            (Ok(text), _) => text.to_owned(),
            (Err(_), Some(enc)) => enc.decode_without_bom_handling(line).0.into_owned(),
            (Err(_), None) => String::from_utf8_lossy(line).into_owned(),
        })
        .collect()
}

/// The system's ANSI code page (read once from the registry with `reg`).
#[cfg(windows)]
fn ansi_encoding() -> Option<&'static encoding_rs::Encoding> {
    use std::os::windows::process::CommandExt;
    static ENCODING: std::sync::OnceLock<Option<&'static encoding_rs::Encoding>> =
        std::sync::OnceLock::new();
    *ENCODING.get_or_init(|| {
        let out = std::process::Command::new("reg")
            .args([
                "query",
                r"HKLM\SYSTEM\CurrentControlSet\Control\Nls\CodePage",
                "/v",
                "ACP",
            ])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000)
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        code_page_encoding(text.split_whitespace().last()?.parse().ok()?)
    })
}

#[cfg(not(windows))]
fn ansi_encoding() -> Option<&'static encoding_rs::Encoding> {
    None
}

/// The encoding of a Windows code page, for the ones Windows uses as ANSI.
#[cfg_attr(not(windows), allow(dead_code))]
fn code_page_encoding(cp: u16) -> Option<&'static encoding_rs::Encoding> {
    let label = match cp {
        950 => "big5".to_owned(),
        936 => "gbk".to_owned(),
        932 => "shift_jis".to_owned(),
        949 => "euc-kr".to_owned(),
        874 | 1250..=1258 => format!("windows-{cp}"),
        _ => return None,
    };
    encoding_rs::Encoding::for_label(label.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(out: &ToolOutput) -> String {
        match &out.content[0] {
            ak_types::chat::ContentPart::Text { text } => text.clone(),
            _ => panic!(),
        }
    }

    #[cfg(windows)]
    const PRINT_CWD: &str = "cd";
    #[cfg(not(windows))]
    const PRINT_CWD: &str = "pwd";
    #[cfg(windows)]
    const PRINT_VAR: &str = "echo %AK_TEST_VAR%";
    #[cfg(not(windows))]
    const PRINT_VAR: &str = "echo $AK_TEST_VAR";
    #[test]
    fn code_page_lines_are_decoded_next_to_utf8_lines() {
        let big5 = code_page_encoding(950).unwrap();
        // `cd` in C:\專案 (Big5), then cargo's UTF-8 line.
        let mut bytes = b"C:\\".to_vec();
        bytes.extend(big5.encode("專案").0.iter());
        bytes.extend("\r\n   Compiling 中文 v0.1.0\n".as_bytes());
        assert_eq!(
            decode_lines(&bytes, Some(big5)),
            "C:\\專案\r\n   Compiling 中文 v0.1.0\n"
        );
        assert_eq!(
            decode_lines("已是 UTF-8".as_bytes(), Some(big5)),
            "已是 UTF-8"
        );
        assert!(decode_lines(&[0xB1, 0x4D], None).contains('\u{FFFD}'));
        assert_eq!(code_page_encoding(1252).unwrap().name(), "windows-1252");
        assert!(code_page_encoding(65001).is_none());
    }

    #[cfg(windows)]
    const SLEEP: &str = "ping -n 30 127.0.0.1 >nul";
    #[cfg(not(windows))]
    const SLEEP: &str = "sleep 30";

    #[tokio::test]
    async fn runs_in_project_with_env_and_exit_codes() {
        let dir = tempfile::tempdir().unwrap();
        let c = ToolContext::for_project(dir.path());
        let tool = RunCommand::new(ToolEnv {
            path_prefix: vec![dir.path().join("toolchain")],
            vars: vec![("AK_TEST_VAR".into(), "你好 agent".into())],
        });
        let out = tool.call(json!({ "command": "echo hello" }), &c).await;
        assert!(!out.is_error);
        assert!(
            text(&out).starts_with("exit code: 0\nhello"),
            "{}",
            text(&out)
        );

        let cwd = text(&tool.call(json!({ "command": PRINT_CWD }), &c).await);
        let name = dir
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(cwd.contains(&name), "{cwd}");

        let var = text(&tool.call(json!({ "command": PRINT_VAR }), &c).await);
        assert!(var.contains("agent"), "env vars reach the command: {var}");
        // cmd's built-in `echo` writes the ANSI code page to pipes (chcp does not
        // change that); external tools such as cargo write UTF-8. Known limitation.
        #[cfg(not(windows))]
        assert!(var.contains("你好 agent"), "UTF-8 output survives: {var}");

        // Regression: quotes must reach the shell unchanged (on Windows, Rust's
        // default argument escaping turned `"` into `\"`, which cmd mangles).
        std::fs::create_dir_all(dir.path().join("my folder")).unwrap();
        std::fs::write(dir.path().join("my folder").join("a.txt"), "inside").unwrap();
        #[cfg(windows)]
        let read_quoted = "type \"my folder\\a.txt\"";
        #[cfg(not(windows))]
        let read_quoted = "cat \"my folder/a.txt\"";
        let quoted = tool.call(json!({ "command": read_quoted }), &c).await;
        assert!(text(&quoted).contains("inside"), "{}", text(&quoted));

        let fail = tool.call(json!({ "command": "exit 3" }), &c).await;
        assert!(fail.is_error && text(&fail).starts_with("exit code: 3"));
    }

    #[tokio::test]
    async fn timeout_and_cancel_stop_the_process() {
        let dir = tempfile::tempdir().unwrap();
        let c = ToolContext::for_project(dir.path());
        let tool = RunCommand::new(ToolEnv::default());
        let started = std::time::Instant::now();
        let out = tool
            .call(json!({ "command": SLEEP, "timeout_secs": 1 }), &c)
            .await;
        assert!(text(&out).contains("stopped after 1 seconds"));
        assert!(started.elapsed() < Duration::from_secs(10));

        let cancel = c.cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            cancel.cancel();
        });
        let started = std::time::Instant::now();
        let out = tool.call(json!({ "command": SLEEP }), &c).await;
        assert!(text(&out).contains("Cancelled"));
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    /// Regression: killing only the shell left its child running and the pipes open.
    #[tokio::test]
    async fn timeout_kills_child_processes_too() {
        let dir = tempfile::tempdir().unwrap();
        let c = ToolContext::for_project(dir.path());
        let tool = RunCommand::new(ToolEnv::default());
        // A grandchild that writes a file 3 s later, unless it was killed with its shell.
        #[cfg(windows)]
        let line = "cmd /C \"ping -n 4 127.0.0.1 >nul & echo late> late.txt\"";
        #[cfg(not(windows))]
        let line = "sh -c 'sleep 3; echo late > late.txt'";
        let out = tool
            .call(json!({ "command": line, "timeout_secs": 1 }), &c)
            .await;
        assert!(text(&out).contains("stopped after"));
        tokio::time::sleep(Duration::from_secs(5)).await;
        assert!(
            !dir.path().join("late.txt").exists(),
            "the child process survived the timeout"
        );
    }
}
