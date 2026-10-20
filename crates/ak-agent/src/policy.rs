//! The default permission policy (SPEC §5, AGENTS.md §4 "Permissions"):
//! - work inside the project runs automatically;
//! - downloads/installs, anything outside the project, and deletions ask the user;
//! - path escapes, edits to `.git`, and commands that could damage the computer are
//!   refused.
//!
//! Command classification is a heuristic over a quote-aware split of the command
//! line; it errs towards asking. The file tools enforce the project sandbox again on
//! their own (`tools::path`).

use std::path::{Component, Path, PathBuf};

use ak_types::agent::{ConfirmKind, ConfirmReason};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::permission::{Decision, PermissionGate};
use crate::tool::ToolContext;
use crate::tools::path::resolve;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    /// Automatic inside the project; ask for risky steps.
    #[default]
    Standard,
    /// Additionally ask before every file change and command.
    AskEveryChange,
}

pub struct ProjectPolicy {
    mode: PermissionMode,
}

impl ProjectPolicy {
    pub fn new(mode: PermissionMode) -> Self {
        Self { mode }
    }
}

fn reason(kind: ConfirmKind, detail: impl Into<String>) -> ConfirmReason {
    ConfirmReason {
        kind,
        detail: detail.into(),
    }
}

fn ask(kind: ConfirmKind, detail: impl Into<String>) -> Decision {
    Decision::Ask {
        reason: reason(kind, detail),
    }
}

fn deny(kind: ConfirmKind, detail: impl Into<String>) -> Decision {
    Decision::Deny {
        reason: reason(kind, detail),
    }
}

fn severity(d: &Decision) -> u8 {
    match d {
        Decision::Allow => 0,
        Decision::Ask { .. } => 1,
        Decision::Deny { .. } => 2,
    }
}

/// The stricter decision; on a tie, the first one.
fn stricter(a: Decision, b: Decision) -> Decision {
    if severity(&b) > severity(&a) { b } else { a }
}

impl PermissionGate for ProjectPolicy {
    fn check(&self, tool: &str, args: &Value, ctx: &ToolContext) -> Decision {
        let Some(root) = ctx.project_dir.as_deref() else {
            // No project: the tools refuse on their own.
            return Decision::Allow;
        };
        let path_arg = args["path"].as_str().unwrap_or(".");
        let change = |d: Decision, detail: &str| match (d, self.mode) {
            (Decision::Allow, PermissionMode::AskEveryChange) => ask(ConfirmKind::Change, detail),
            (d, _) => d,
        };
        match tool {
            "read_file" | "list_files" | "search_files" => file_decision(root, path_arg, false),
            "write_file" | "edit_file" => change(file_decision(root, path_arg, true), path_arg),
            "run_command" => {
                let cmd = args["command"].as_str().unwrap_or_default();
                change(classify_command(cmd, root), cmd)
            }
            "ask_user" => Decision::Allow,
            // A tool this policy does not know: ask rather than guess.
            other => ask(ConfirmKind::Unrecognized, other),
        }
    }
}

fn file_decision(root: &Path, rel: &str, writes: bool) -> Decision {
    match resolve(root, rel) {
        Err(_) => deny(ConfirmKind::PathEscape, rel),
        Ok(path) if writes && path.starts_with(root.join(".git")) => {
            deny(ConfirmKind::ProtectedPath, rel)
        }
        Ok(_) => Decision::Allow,
    }
}

/// Classifies a shell command line. The strictest segment decides.
pub fn classify_command(command: &str, root: &Path) -> Decision {
    split_segments(command)
        .iter()
        .map(|seg| classify_segment(seg, root, 0))
        .fold(Decision::Allow, stricter)
}

const DANGEROUS: &[&str] = &[
    "format", "diskpart", "mkfs", "shutdown", "reboot", "halt", "poweroff", "bcdedit", "dd",
    "fdisk", "parted",
];
const DELETERS: &[&str] = &[
    "rm",
    "rmdir",
    "rd",
    "del",
    "erase",
    "unlink",
    "shred",
    "remove-item",
    "ri",
    "rimraf",
];
const DOWNLOADERS: &[&str] = &[
    "curl",
    "wget",
    "invoke-webrequest",
    "iwr",
    "invoke-restmethod",
    "irm",
    "bitsadmin",
    "winget",
    "choco",
    "scoop",
    "brew",
    "apt",
    "apt-get",
    "dnf",
    "yum",
    "pacman",
    "snap",
    "pip",
    "pip3",
    "pipx",
    "npm",
    "npx",
    "pnpm",
    "yarn",
    "bun",
    "gem",
];
const ELEVATORS: &[&str] = &["sudo", "doas", "runas", "su", "gsudo"];
const SHELLS: &[&str] = &["cmd", "sh", "bash", "zsh", "powershell", "pwsh"];

fn classify_segment(tokens: &[String], root: &Path, depth: u8) -> Decision {
    let mut tokens: &[String] = tokens;
    // Skip `KEY=value` environment prefixes.
    while tokens.first().is_some_and(|t| is_env_assignment(t)) {
        tokens = &tokens[1..];
    }
    let Some(first) = tokens.first() else {
        return Decision::Allow;
    };
    let word = command_word(first);
    let rest = &tokens[1..];
    let lower: Vec<String> = rest.iter().map(|t| t.to_ascii_lowercase()).collect();
    let sub = lower.first().map(String::as_str).unwrap_or("");
    let full = tokens.join(" ");

    let mut decision = paths_outside(tokens, root)
        .map_or(Decision::Allow, |p| ask(ConfirmKind::OutsideProject, p));

    if ELEVATORS.contains(&word.as_str()) {
        let inner = if depth < 3 {
            classify_segment(rest, root, depth + 1)
        } else {
            Decision::Allow
        };
        return stricter(stricter(ask(ConfirmKind::Elevated, &full), inner), decision);
    }
    if SHELLS.contains(&word.as_str()) {
        if lower
            .iter()
            .any(|t| matches!(t.as_str(), "-encodedcommand" | "-enc" | "-e" | "-ec"))
        {
            return stricter(ask(ConfirmKind::Unrecognized, &full), decision);
        }
        // `cmd /c …`, `sh -c "…"`, `powershell -Command …`: look inside.
        if let Some(i) = lower
            .iter()
            .position(|t| matches!(t.as_str(), "/c" | "/k" | "-c" | "-command"))
            && depth < 3
        {
            let inner = rest[i + 1..].join(" ");
            let nested = split_segments(&inner)
                .iter()
                .map(|s| classify_segment(s, root, depth + 1))
                .fold(Decision::Allow, stricter);
            return stricter(nested, decision);
        }
    }

    let dangerous = DANGEROUS
        .iter()
        .any(|d| word == *d || word.starts_with("mkfs."))
        || (word == "reg" && sub == "delete")
        || (DELETERS.contains(&word.as_str()) && lower.iter().any(|t| is_filesystem_root(t)));
    if dangerous {
        return deny(ConfirmKind::Dangerous, &full);
    }

    let deletes = DELETERS.contains(&word.as_str())
        || (word == "git"
            && (sub == "clean"
                || sub == "restore"
                || (sub == "reset" && lower.iter().any(|t| t == "--hard"))
                || (sub == "checkout" && lower.iter().any(|t| t == "--" || t == "."))
                || (sub == "stash" && lower.get(1).is_some_and(|t| t == "drop" || t == "clear"))));
    if deletes {
        decision = stricter(decision, ask(ConfirmKind::Delete, &full));
    }

    let downloads = DOWNLOADERS.contains(&word.as_str())
        || (word == "cargo" && matches!(sub, "install" | "binstall"))
        || (word == "rustup" && !matches!(sub, "show" | "which" | "--version" | "-v" | "doc"))
        || (word == "git" && sub == "clone")
        || (word == "go" && matches!(sub, "install" | "get"))
        || (word == "certutil" && lower.iter().any(|t| t == "-urlcache"));
    if downloads {
        decision = stricter(decision, ask(ConfirmKind::Download, &full));
    }
    decision
}

fn is_env_assignment(t: &str) -> bool {
    t.split_once('=').is_some_and(|(k, _)| {
        !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// `C:\tools\Git\bin\rm.exe` -> `rm`
fn command_word(t: &str) -> String {
    let name = t
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(t)
        .to_ascii_lowercase();
    for ext in [".exe", ".cmd", ".bat", ".com", ".ps1"] {
        if let Some(stem) = name.strip_suffix(ext) {
            return stem.to_owned();
        }
    }
    name
}

fn is_filesystem_root(t: &str) -> bool {
    let t = t.trim_end_matches(['*']);
    matches!(t, "/" | "~" | "~/" | "\\" | "/*")
        || (t.len() <= 3
            && t.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
            && t.get(1..2) == Some(":"))
}

/// The first argument that points outside the project, if any.
fn paths_outside(tokens: &[String], root: &Path) -> Option<String> {
    tokens.iter().find_map(|tok| {
        // `--out=../x`, `>../log.txt`, `2>>C:\x` -> the path part
        let t = tok
            .trim_start_matches(['0', '1', '2'])
            .trim_start_matches('>');
        let t = t.rsplit_once('=').map_or(t, |(_, v)| v);
        is_outside(t, root).then(|| tok.clone())
    })
}

fn is_outside(t: &str, root: &Path) -> bool {
    if t.is_empty() || t.contains("://") || t == "/dev/null" || t.eq_ignore_ascii_case("nul") {
        return false;
    }
    let lower = t.to_ascii_lowercase();
    const ENV_DIRS: &[&str] = &[
        "%userprofile%",
        "%appdata%",
        "%localappdata%",
        "%temp%",
        "%tmp%",
        "%systemroot%",
        "%windir%",
        "%programfiles%",
        "%programdata%",
        "%homepath%",
        "$home",
        "${home}",
        "$env:",
    ];
    if t.starts_with('~') || ENV_DIRS.iter().any(|e| lower.contains(e)) {
        return true;
    }
    let bytes = t.as_bytes();
    let drive_abs = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/');
    let unc = t.starts_with("\\\\");
    // On Windows, `/x` is usually a command flag (`dir /b`, `rd /s`), not a path.
    let unix_abs = t.starts_with('/') && (!cfg!(windows) || t[1..].contains(['/', '\\']));
    if drive_abs || unc || unix_abs {
        return !within(root, Path::new(t));
    }
    // Relative: does `..` climb out of the project?
    if t.contains("..") {
        let mut depth: i32 = 0;
        for c in Path::new(t).components() {
            match c {
                Component::ParentDir => depth -= 1,
                Component::Normal(_) => depth += 1,
                _ => {}
            }
            if depth < 0 {
                return true;
            }
        }
    }
    false
}

/// Whether an absolute path lies inside `root` (case-insensitive on Windows).
fn within(root: &Path, path: &Path) -> bool {
    let norm = |p: &Path| -> PathBuf {
        let s = p.to_string_lossy().replace('\\', "/");
        PathBuf::from(if cfg!(windows) {
            s.to_ascii_lowercase()
        } else {
            s
        })
    };
    let root_real = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let candidates = [norm(root), norm(&root_real)];
    let p = norm(path);
    candidates.iter().any(|r| {
        let r = r.to_string_lossy();
        let r = r.trim_start_matches("//?/");
        p.to_string_lossy().starts_with(r)
    })
}

/// Splits a command line into segments of tokens on unquoted `&&`, `||`, `;`, `|`,
/// `&` and newlines (not the `&` of `2>&1`). Quotes are removed from tokens;
/// backslashes are literal (Windows paths).
pub fn split_segments(command: &str) -> Vec<Vec<String>> {
    let mut segments = Vec::new();
    let mut tokens: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut in_token = false;
    let mut quote: Option<char> = None;
    let chars: Vec<char> = command.chars().collect();
    let mut i = 0;
    let push_token = |tokens: &mut Vec<String>, cur: &mut String, in_token: &mut bool| {
        if *in_token {
            tokens.push(std::mem::take(cur));
            *in_token = false;
        }
    };
    while i < chars.len() {
        let c = chars[i];
        if let Some(q) = quote {
            if c == q {
                quote = None;
            } else {
                cur.push(c);
            }
            i += 1;
            continue;
        }
        match c {
            '"' | '\'' => {
                quote = Some(c);
                in_token = true;
            }
            '&' if cur.ends_with('>') => {
                // `2>&1`: part of a redirection.
                cur.push(c);
            }
            '&' | '|' | ';' | '\n' => {
                push_token(&mut tokens, &mut cur, &mut in_token);
                if !tokens.is_empty() {
                    segments.push(std::mem::take(&mut tokens));
                }
                // `&&`, `||`
                if (c == '&' || c == '|') && chars.get(i + 1) == Some(&c) {
                    i += 1;
                }
            }
            c if c.is_whitespace() => push_token(&mut tokens, &mut cur, &mut in_token),
            c => {
                cur.push(c);
                in_token = true;
            }
        }
        i += 1;
    }
    push_token(&mut tokens, &mut cur, &mut in_token);
    if !tokens.is_empty() {
        segments.push(tokens);
    }
    segments
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn kind(d: &Decision) -> String {
        match d {
            Decision::Allow => "allow".into(),
            Decision::Ask { reason } => format!("ask:{:?}", reason.kind),
            Decision::Deny { reason } => format!("deny:{:?}", reason.kind),
        }
    }

    #[test]
    fn splitting() {
        assert_eq!(
            split_segments(r#"cargo build 2>&1 && echo "a && b" | grep x; trunk serve"#),
            vec![
                vec!["cargo", "build", "2>&1"],
                vec!["echo", "a && b"],
                vec!["grep", "x"],
                vec!["trunk", "serve"],
            ]
        );
        assert_eq!(
            split_segments("type \"my folder\\a.txt\""),
            vec![vec!["type", "my folder\\a.txt"]]
        );
    }

    /// (tool, args, expected) for every rule.
    #[test]
    fn decision_table() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        let ctx = ToolContext::for_project(dir.path());
        let policy = ProjectPolicy::new(PermissionMode::Standard);
        let cases: Vec<(&str, Value, &str)> = vec![
            // File tools
            ("read_file", json!({ "path": "src/app.rs" }), "allow"),
            ("list_files", json!({}), "allow"),
            (
                "search_files",
                json!({ "pattern": "x", "path": "src" }),
                "allow",
            ),
            (
                "write_file",
                json!({ "path": "src/new.rs", "content": "" }),
                "allow",
            ),
            ("edit_file", json!({ "path": "Cargo.toml" }), "allow"),
            (
                "read_file",
                json!({ "path": "../secrets.txt" }),
                "deny:PathEscape",
            ),
            (
                "write_file",
                json!({ "path": "/etc/hosts" }),
                "deny:PathEscape",
            ),
            (
                "write_file",
                json!({ "path": ".git/config" }),
                "deny:ProtectedPath",
            ),
            ("read_file", json!({ "path": ".git/HEAD" }), "allow"),
            ("ask_user", json!({}), "allow"),
            ("brand_new_tool", json!({}), "ask:Unrecognized"),
            // Routine commands
            (
                "run_command",
                json!({ "command": "cargo build --release" }),
                "allow",
            ),
            (
                "run_command",
                json!({ "command": "cargo add serde && cargo test 2>&1" }),
                "allow",
            ),
            ("run_command", json!({ "command": "trunk build" }), "allow"),
            ("run_command", json!({ "command": "git status" }), "allow"),
            (
                "run_command",
                json!({ "command": "dir /b /s src" }),
                "allow",
            ),
            (
                "run_command",
                json!({ "command": "echo hi > out/log.txt" }),
                "allow",
            ),
            ("run_command", json!({ "command": "rustup show" }), "allow"),
            (
                "run_command",
                json!({ "command": format!("type \"{root}/Cargo.toml\"") }),
                "allow",
            ),
            (
                "run_command",
                json!({ "command": "curl_helper_is_not_curl.sh" }),
                "allow",
            ),
            // Downloads and installs
            (
                "run_command",
                json!({ "command": "cargo install trunk" }),
                "ask:Download",
            ),
            (
                "run_command",
                json!({ "command": "rustup target add wasm32-unknown-unknown" }),
                "ask:Download",
            ),
            (
                "run_command",
                json!({ "command": "curl -L https://example.com/x.zip -o x.zip" }),
                "ask:Download",
            ),
            (
                "run_command",
                json!({ "command": "winget install Git.Git" }),
                "ask:Download",
            ),
            (
                "run_command",
                json!({ "command": "npm install" }),
                "ask:Download",
            ),
            (
                "run_command",
                json!({ "command": "git clone https://github.com/a/b" }),
                "ask:Download",
            ),
            (
                "run_command",
                json!({ "command": "C:\\tools\\curl.exe https://x" }),
                "ask:OutsideProject",
            ),
            // Deletions
            (
                "run_command",
                json!({ "command": "rm -rf dist" }),
                "ask:Delete",
            ),
            (
                "run_command",
                json!({ "command": "del /q old.txt" }),
                "ask:Delete",
            ),
            (
                "run_command",
                json!({ "command": "rd /s /q build" }),
                "ask:Delete",
            ),
            (
                "run_command",
                json!({ "command": "Remove-Item -Recurse temp" }),
                "ask:Delete",
            ),
            (
                "run_command",
                json!({ "command": "git reset --hard HEAD~1" }),
                "ask:Delete",
            ),
            (
                "run_command",
                json!({ "command": "git clean -fdx" }),
                "ask:Delete",
            ),
            (
                "run_command",
                json!({ "command": "cargo build && rm target/x" }),
                "ask:Delete",
            ),
            (
                "run_command",
                json!({ "command": "cmd /c \"del a.txt\"" }),
                "ask:Delete",
            ),
            // Outside the project
            (
                "run_command",
                json!({ "command": "type ..\\..\\secrets.txt" }),
                "ask:OutsideProject",
            ),
            (
                "run_command",
                json!({ "command": "cd .." }),
                "ask:OutsideProject",
            ),
            (
                "run_command",
                json!({ "command": "copy a.txt %USERPROFILE%\\Desktop" }),
                "ask:OutsideProject",
            ),
            (
                "run_command",
                json!({ "command": "cat ~/.ssh/id_rsa" }),
                "ask:OutsideProject",
            ),
            (
                "run_command",
                json!({ "command": "type C:\\Windows\\win.ini" }),
                "ask:OutsideProject",
            ),
            (
                "run_command",
                json!({ "command": "cargo build --target-dir=../shared" }),
                "ask:OutsideProject",
            ),
            // Elevated and opaque
            (
                "run_command",
                json!({ "command": "sudo apt-get install foo" }),
                "ask:Elevated",
            ),
            (
                "run_command",
                json!({ "command": "powershell -EncodedCommand ZQBjAGgAbwA=" }),
                "ask:Unrecognized",
            ),
            // Dangerous: refused outright
            (
                "run_command",
                json!({ "command": "format D: /q" }),
                "deny:Dangerous",
            ),
            (
                "run_command",
                json!({ "command": "rm -rf /" }),
                "deny:Dangerous",
            ),
            (
                "run_command",
                json!({ "command": "rd /s /q C:\\" }),
                "deny:Dangerous",
            ),
            (
                "run_command",
                json!({ "command": "echo ok && shutdown /s /t 0" }),
                "deny:Dangerous",
            ),
            (
                "run_command",
                json!({ "command": "sudo rm -rf /" }),
                "deny:Dangerous",
            ),
            (
                "run_command",
                json!({ "command": "reg delete HKCU\\Software\\X /f" }),
                "deny:Dangerous",
            ),
        ];
        let mut failures = Vec::new();
        for (tool, args, expected) in &cases {
            let got = kind(&policy.check(tool, args, &ctx));
            if got != *expected {
                failures.push(format!("{tool} {args}: expected {expected}, got {got}"));
            }
        }
        assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    }

    #[test]
    fn ask_every_change_mode() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolContext::for_project(dir.path());
        let policy = ProjectPolicy::new(PermissionMode::AskEveryChange);
        let check = |tool: &str, args: Value| kind(&policy.check(tool, &args, &ctx));
        assert_eq!(check("read_file", json!({ "path": "a" })), "allow");
        assert_eq!(check("write_file", json!({ "path": "a" })), "ask:Change");
        assert_eq!(
            check("run_command", json!({ "command": "cargo build" })),
            "ask:Change"
        );
        // Stricter reasons still win.
        assert_eq!(
            check("run_command", json!({ "command": "rm x" })),
            "ask:Delete"
        );
        assert_eq!(
            check("write_file", json!({ "path": "../x" })),
            "deny:PathEscape"
        );
    }
}
