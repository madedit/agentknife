//! The default permission policy (SPEC §5, AGENTS.md §4 "Permissions"):
//! - work inside the project runs automatically;
//! - reading the reference folders (library sources, folders the user allowed) too,
//!   with the read tools or with commands that only read;
//! - downloads/installs, anything outside the project, and deletions ask the user;
//! - path escapes, edits to `.git`, `.agentknife/` state and `ak/` (with the file
//!   tools or a command that names them), and commands that could damage the
//!   computer are refused.
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
use crate::tools::path::{is_outside_refusal, resolve, resolve_read};

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
    /// Read-only folders outside the project (library sources, folders the user allowed).
    references: Vec<PathBuf>,
}

impl ProjectPolicy {
    pub fn new(mode: PermissionMode) -> Self {
        Self {
            mode,
            references: Vec::new(),
        }
    }

    /// Lets the read tools and read-only commands use these folders without
    /// asking. They hold public library code: no user data, no keys.
    pub fn with_reference_dirs(mut self, dirs: Vec<PathBuf>) -> Self {
        self.references = dirs;
        self
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
            "read_file" | "list_files" | "search_files" => {
                match resolve_read(root, &self.references, path_arg) {
                    Err(e) if is_outside_refusal(&e) => deny(ConfirmKind::PathEscape, path_arg),
                    // A missing file: the tool resolves again and says what is wrong.
                    _ => Decision::Allow,
                }
            }
            "write_file" | "edit_file" => change(file_decision(root, path_arg, true), path_arg),
            // Copies a library that ships with AgentKnife into its fixed folder.
            crate::tools::ADD_LIBRARY => change(Decision::Allow, crate::tools::ADD_LIBRARY),
            "run_command" => {
                let cmd = args["command"].as_str().unwrap_or_default();
                change(classify_with(cmd, root, &self.references), cmd)
            }
            // Reads AgentKnife's built-in docs; touches nothing.
            "ask_user" | crate::tools::READ_KNOWLEDGE => Decision::Allow,
            // Looks at or uses the running preview; never touches project files.
            t if crate::tools::PREVIEW_TOOLS.contains(&t) => Decision::Allow,
            // A tool this policy does not know: ask rather than guess.
            other => ask(ConfirmKind::Unrecognized, other),
        }
    }
}

/// AgentKnife's own state: the user's `.git`, the `.agentknife/` internals (the
/// agent may still edit `.agentknife/PROJECT.md` and `plan/`), and the `ak/` kit
/// (ak-bridge, ak-ui), which AgentKnife updates.
const PROTECTED: &[&str] = &[
    ".git",
    "ak",
    ".agentknife/checkpoints.git",
    ".agentknife/timeline.json",
    ".agentknife/chat",
    ".agentknife/turn.lock",
    ".agentknife/project.json",
];

fn file_decision(root: &Path, rel: &str, writes: bool) -> Decision {
    match resolve(root, rel) {
        Err(_) => deny(ConfirmKind::PathEscape, rel),
        Ok(path) if writes && PROTECTED.iter().any(|p| path.starts_with(root.join(p))) => {
            deny(ConfirmKind::ProtectedPath, rel)
        }
        Ok(_) => Decision::Allow,
    }
}

/// Classifies a shell command line. The strictest segment decides.
pub fn classify_command(command: &str, root: &Path) -> Decision {
    classify_with(command, root, &[])
}

/// [`classify_command`] where commands that only read may use `references`.
pub fn classify_with(command: &str, root: &Path, references: &[PathBuf]) -> Decision {
    split_segments(command)
        .iter()
        .map(|seg| classify_segment(seg, root, references, 0))
        .fold(Decision::Allow, stricter)
}

/// Commands that only read or list (no writing, no deleting, no running other
/// programs). `find` is not here: POSIX `find` can `-delete` and `-exec`.
const READERS: &[&str] = &[
    "type",
    "cat",
    "more",
    "less",
    "head",
    "tail",
    "findstr",
    "grep",
    "egrep",
    "fgrep",
    "rg",
    "dir",
    "ls",
    "tree",
    "wc",
    "get-content",
    "gc",
    "select-string",
    "sls",
    "get-childitem",
    "gci",
    "get-item",
    "gi",
    "test-path",
    "resolve-path",
];

/// `>file`, `2>>file`: the command writes there.
fn is_redirect(t: &str) -> bool {
    t.trim_start_matches(['0', '1', '2', '*']).starts_with('>')
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

fn classify_segment(tokens: &[String], root: &Path, refs: &[PathBuf], depth: u8) -> Decision {
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

    // Reading reference folders needs no OK; anything else outside the project does.
    let reads_references = READERS.contains(&word.as_str())
        && !tokens.iter().any(|t| is_redirect(t))
        && outside_paths(tokens, root)
            .iter()
            .all(|(_, p)| refs.iter().any(|r| within(r, Path::new(p))));
    // A command that names a protected path writes there unless it only reads
    // (shells and `sudo` are looked into below instead).
    if !SHELLS.contains(&word.as_str())
        && !ELEVATORS.contains(&word.as_str())
        && let Some(t) = protected_target(&word, tokens, root)
    {
        return deny(ConfirmKind::ProtectedPath, t);
    }
    let mut decision = match paths_outside(tokens, root) {
        Some(p) if !reads_references => ask(ConfirmKind::OutsideProject, p),
        _ => Decision::Allow,
    };

    if ELEVATORS.contains(&word.as_str()) {
        let inner = if depth < 3 {
            classify_segment(rest, root, refs, depth + 1)
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
                .map(|s| classify_segment(s, root, refs, depth + 1))
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

/// The first token of a command that may write a [`PROTECTED`] path: any token
/// naming one, or for [`READERS`] only where their output is redirected.
fn protected_target(word: &str, tokens: &[String], root: &Path) -> Option<String> {
    let reader = READERS.contains(&word);
    tokens
        .iter()
        .enumerate()
        .find(|(i, t)| {
            // An inline script's first token may be the write (`$p='ak/…'`).
            let after_redirect = i
                .checked_sub(1)
                .and_then(|j| tokens.get(j))
                .is_some_and(|prev| is_redirect(prev) && prev.ends_with('>'));
            let written = !reader || is_redirect(t) || after_redirect;
            written && names_protected(t, root)
        })
        .map(|(_, t)| t.clone())
}

/// Whether a token names one of [`PROTECTED`]: `ak/ak-ui/src/x.rs`,
/// `ak\ak-ui`, `.git/config`, `$p=ak/…`, `>ak/x`, or the same under the
/// project's absolute path. `.gitignore` and `akira/` are not.
fn names_protected(token: &str, root: &Path) -> bool {
    let root = root
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    let root = root.trim_start_matches("//?/").trim_end_matches('/');
    let t = token.replace('\\', "/").to_ascii_lowercase();
    t.split(['=', '\'', '"', '(', ')', ',', '`', '>', '<'])
        .map(|p| {
            let p = p
                .strip_prefix(root)
                .map_or(p, |r| r.trim_start_matches('/'));
            p.trim_start_matches("./")
        })
        .any(|p| {
            PROTECTED
                .iter()
                .any(|prot| p == *prot || p.strip_prefix(prot).is_some_and(|r| r.starts_with('/')))
        })
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
    outside_paths(tokens, root)
        .into_iter()
        .next()
        .map(|(tok, _)| tok)
}

/// Every argument that points outside the project: (the token, its path part).
fn outside_paths(tokens: &[String], root: &Path) -> Vec<(String, String)> {
    tokens
        .iter()
        .filter_map(|tok| {
            // `--out=../x`, `>../log.txt`, `2>>C:\x` -> the path part
            let t = tok
                .trim_start_matches(['0', '1', '2'])
                .trim_start_matches('>');
            let t = t.rsplit_once('=').map_or(t, |(_, v)| v);
            is_outside(t, root).then(|| (tok.clone(), t.to_owned()))
        })
        .collect()
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
    fn commands_cannot_write_protected_paths() {
        let project = tempfile::tempdir().unwrap();
        let root = project.path();
        let check = |c: &str| kind(&classify_command(c, root));
        // What an AI ran after edit_file was refused (2026-10-28).
        let script = r#"powershell -NoProfile -Command "$p='ak/ak-ui/src/button.rs'; $c=[IO.File]::ReadAllText($p); $c=$c -replace 'rounded-lg','rounded-full'; [IO.File]::WriteAllText($p,$c)" && findstr /n /c:"rounded-" ak\ak-ui\src\button.rs"#;
        assert_eq!(check(script), "deny:ProtectedPath");
        for c in [
            r"copy src\button.rs ak\ak-ui\src\button.rs",
            "sed -i s/lg/full/ ./ak/ak-ui/src/button.rs",
            "echo x > ak/ak-ui/src/lib.rs",
            "type src\\a.rs >> .git\\config",
            "cd ak",
            "del .agentknife\\timeline.json",
        ] {
            assert_eq!(check(c), "deny:ProtectedPath", "{c}");
        }
        let absolute = format!(
            "echo x > {}",
            root.join("ak").join("ak-ui").join("x.rs").display()
        );
        assert_eq!(check(&absolute), "deny:ProtectedPath");
        // Reading them, and names that only look alike, are fine. (findstr's
        // `/c:` is an absolute path elsewhere: outside the project.)
        if cfg!(windows) {
            let c = r#"findstr /n /c:"rounded-" ak\ak-ui\src\button.rs"#;
            assert_eq!(check(c), "allow", "{c}");
        }
        for c in [
            "type ak\\ak-ui\\src\\list.rs > notes.txt",
            "echo target/ >> .gitignore",
            "copy notes.md akira\\notes.md",
            "cargo build --features dev",
            "git status",
        ] {
            assert_eq!(check(c), "allow", "{c}");
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

    #[test]
    fn library_sources_can_be_read_without_asking() {
        let project = tempfile::tempdir().unwrap();
        let lib = tempfile::tempdir().unwrap();
        let src = lib.path().join("tauri-utils").join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("lib.rs"), "pub fn x() {}").unwrap();
        let file = src.join("lib.rs").to_string_lossy().into_owned();
        let folder = src.to_string_lossy().into_owned();
        let ctx = ToolContext::for_project(project.path());
        let refs = vec![lib.path().to_path_buf()];
        let policy = ProjectPolicy::new(PermissionMode::Standard).with_reference_dirs(refs.clone());
        let plain = ProjectPolicy::new(PermissionMode::Standard);
        let cmd = |p: &ProjectPolicy, c: &str| {
            kind(&p.check("run_command", &json!({ "command": c }), &ctx))
        };
        let tool = |p: &ProjectPolicy, t: &str, path: &str| {
            kind(&p.check(t, &json!({ "path": path, "pattern": "x" }), &ctx))
        };

        // Reading: no question with the reference folder, the old question without.
        for c in [
            format!("type \"{file}\""),
            format!("rg -n \"fn x\" \"{folder}\""),
            format!("Get-Content \"{file}\" | Select-String x"),
            if cfg!(windows) {
                format!("findstr /n resolve \"{file}\"")
            } else {
                format!("grep -n resolve \"{file}\"")
            },
        ] {
            assert_eq!(cmd(&policy, &c), "allow", "{c}");
            assert_eq!(cmd(&plain, &c), "ask:OutsideProject", "{c}");
        }
        for t in ["read_file", "list_files", "search_files"] {
            let path = if t == "read_file" { &file } else { &folder };
            assert_eq!(tool(&policy, t, path), "allow", "{t}");
            assert_eq!(tool(&plain, t, path), "deny:PathEscape", "{t}");
        }
        // A wrong folder name there is the tool's to report ("does not exist").
        let missing = lib
            .path()
            .join("tauri-9.9.9")
            .to_string_lossy()
            .into_owned();
        assert_eq!(tool(&policy, "search_files", &missing), "allow");
        // Writing, deleting or redirecting there still asks; other folders too.
        for c in [
            format!("copy src\\a.rs \"{folder}\""),
            format!("del \"{file}\""),
            format!("type a.txt > \"{file}\""),
            "type C:\\Windows\\win.ini".to_owned(),
        ] {
            assert_ne!(cmd(&policy, &c), "allow", "{c}");
        }
        assert_eq!(tool(&policy, "write_file", &file), "deny:PathEscape");
        // In "ask before every change" mode, commands still ask (as before).
        let strict = ProjectPolicy::new(PermissionMode::AskEveryChange).with_reference_dirs(refs);
        assert_eq!(cmd(&strict, &format!("type \"{file}\"")), "ask:Change");
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
            (
                "edit_file",
                json!({ "path": "ak/ak-ui/src/button.rs" }),
                "deny:ProtectedPath",
            ),
            (
                "write_file",
                json!({ "path": "ak/ak-bridge/Cargo.toml" }),
                "deny:ProtectedPath",
            ),
            (
                "read_file",
                json!({ "path": "ak/ak-ui/src/list.rs" }),
                "allow",
            ),
            ("write_file", json!({ "path": "akira/notes.md" }), "allow"),
            (
                "edit_file",
                json!({ "path": ".agentknife/timeline.json" }),
                "deny:ProtectedPath",
            ),
            (
                "write_file",
                json!({ "path": ".agentknife/chat/conversation.json" }),
                "deny:ProtectedPath",
            ),
            (
                "write_file",
                json!({ "path": ".agentknife/checkpoints.git/HEAD" }),
                "deny:ProtectedPath",
            ),
            (
                "edit_file",
                json!({ "path": ".agentknife/PROJECT.md" }),
                "allow",
            ),
            (
                "write_file",
                json!({ "path": ".agentknife/plan/STATUS.md" }),
                "allow",
            ),
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
        // cmd: `/x` is a flag there and `\` a separator.
        let cmd_cases: Vec<(&str, Value, &str)> = if cfg!(windows) {
            vec![
                (
                    "run_command",
                    json!({ "command": "dir /b /s src" }),
                    "allow",
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
                    json!({ "command": "type ..\\..\\secrets.txt" }),
                    "ask:OutsideProject",
                ),
            ]
        } else {
            Vec::new()
        };
        let mut failures = Vec::new();
        for (tool, args, expected) in cases.iter().chain(&cmd_cases) {
            let got = kind(&policy.check(tool, args, &ctx));
            if got != *expected {
                failures.push(format!("{tool} {args}: expected {expected}, got {got}"));
            }
        }
        assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    }

    #[test]
    fn adding_a_library_is_an_ordinary_change() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolContext::for_project(dir.path());
        let policy = ProjectPolicy::new(PermissionMode::Standard);
        let args = json!({ "library": "three" });
        assert_eq!(kind(&policy.check("add_library", &args, &ctx)), "allow");
    }

    #[test]
    fn ask_every_change_mode() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolContext::for_project(dir.path());
        let policy = ProjectPolicy::new(PermissionMode::AskEveryChange);
        let check = |tool: &str, args: Value| kind(&policy.check(tool, &args, &ctx));
        assert_eq!(check("read_file", json!({ "path": "a" })), "allow");
        // Reading the built-in docs never needs asking, even in the strictest mode.
        assert_eq!(
            check("read_knowledge", json!({ "topic": "ak-ui" })),
            "allow"
        );
        assert_eq!(check("write_file", json!({ "path": "a" })), "ask:Change");
        assert_eq!(
            check("add_library", json!({ "library": "three" })),
            "ask:Change"
        );
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
