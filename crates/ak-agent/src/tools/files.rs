//! read_file, write_file, edit_file, list_files.

use ak_types::chat::ToolSpec;
use futures_util::future::BoxFuture;
use serde_json::{Value, json};

use super::path::{display, resolve, resolve_read};
use super::{MAX_OUTPUT_CHARS, SKIP_DIRS, cap, project_root, str_arg};
use crate::tool::{Tool, ToolContext, ToolOutput};

const DEFAULT_READ_LINES: usize = 2_000;

fn read_text(path: &std::path::Path) -> Result<String, ToolOutput> {
    if path.is_dir() {
        return Err(ToolOutput::error(
            "That is a folder. Use list_files to see what is inside.",
        ));
    }
    let bytes =
        std::fs::read(path).map_err(|e| ToolOutput::error(format!("Cannot read the file: {e}")))?;
    String::from_utf8(bytes).map_err(|_| ToolOutput::error("This is not a text file."))
}

pub struct ReadFile;

impl Tool for ReadFile {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "read_file".into(),
            description: "Read a text file in the project, or a library's source in \
                          AgentKnife's toolchain folder (an absolute path; read-only). Long \
                          files are returned in pages of lines; use `offset` to continue."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Project-relative path, e.g. src/app.rs (or an absolute path in a library source or a folder the user allowed)" },
                    "offset": { "type": "integer", "description": "First line to return (1-based). Default 1." },
                    "limit": { "type": "integer", "description": "Maximum number of lines. Default 2000." },
                },
                "required": ["path"],
            }),
        }
    }

    fn call<'a>(&'a self, args: Value, ctx: &'a ToolContext) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            let run = || -> Result<ToolOutput, ToolOutput> {
                let root = project_root(ctx)?;
                let path = resolve_read(root, &ctx.reference_dirs, str_arg(&args, "path")?)
                    .map_err(ToolOutput::error)?;
                let text = read_text(&path)?;
                let lines: Vec<&str> = text.lines().collect();
                let offset = args["offset"].as_u64().map_or(1, |n| n.max(1) as usize);
                let limit = args["limit"]
                    .as_u64()
                    .map_or(DEFAULT_READ_LINES, |n| n.max(1) as usize);
                let start = (offset - 1).min(lines.len());
                let end = (start + limit).min(lines.len());
                let mut out = lines[start..end].join("\n");
                if start > 0 || end < lines.len() {
                    out.push_str(&format!(
                        "\n\n(lines {}–{} of {}; use offset to read more)",
                        start + 1,
                        end,
                        lines.len()
                    ));
                }
                Ok(ToolOutput::text(cap(&out, MAX_OUTPUT_CHARS)))
            };
            run().unwrap_or_else(|e| e)
        })
    }
}

pub struct WriteFile;

impl Tool for WriteFile {
    fn changes_project(&self) -> bool {
        true
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "write_file".into(),
            description: "Create a file or replace its whole content. For small changes to an \
                          existing file prefer edit_file."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Project-relative path" },
                    "content": { "type": "string", "description": "The complete new file content" },
                },
                "required": ["path", "content"],
            }),
        }
    }

    fn call<'a>(&'a self, args: Value, ctx: &'a ToolContext) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            let run = || -> Result<ToolOutput, ToolOutput> {
                let root = project_root(ctx)?;
                let path = resolve(root, str_arg(&args, "path")?).map_err(ToolOutput::error)?;
                let content = str_arg(&args, "content")?;
                if path.is_dir() {
                    return Err(ToolOutput::error("That path is a folder."));
                }
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| ToolOutput::error(format!("Cannot create the folder: {e}")))?;
                }
                std::fs::write(&path, content)
                    .map_err(|e| ToolOutput::error(format!("Cannot write the file: {e}")))?;
                let shown = display(root, &path);
                Ok(ToolOutput {
                    touched: vec![shown.clone()],
                    ..ToolOutput::text(format!(
                        "Wrote {shown} ({} lines).",
                        content.lines().count()
                    ))
                })
            };
            run().unwrap_or_else(|e| e)
        })
    }
}

pub struct EditFile;

impl Tool for EditFile {
    fn changes_project(&self) -> bool {
        true
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "edit_file".into(),
            description: "Replace exact text in a file. `old_text` must match the file exactly \
                          (copy it from read_file) and appear once, unless `replace_all` is true. \
                          To remove text, set `delete` instead of an empty `new_text`."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Project-relative path" },
                    "old_text": { "type": "string", "description": "Exact text to replace, with enough surrounding lines to be unique" },
                    "new_text": { "type": "string", "description": "Replacement text (not empty)" },
                    "delete": { "type": "boolean", "description": "Remove `old_text` (with `new_text` empty). Default false." },
                    "replace_all": { "type": "boolean", "description": "Replace every occurrence. Default false." },
                },
                "required": ["path", "old_text", "new_text"],
            }),
        }
    }

    fn call<'a>(&'a self, args: Value, ctx: &'a ToolContext) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            let run = || -> Result<ToolOutput, ToolOutput> {
                let root = project_root(ctx)?;
                let path = resolve(root, str_arg(&args, "path")?).map_err(ToolOutput::error)?;
                let mut old = str_arg(&args, "old_text")?.to_owned();
                let mut new = str_arg(&args, "new_text")?.to_owned();
                let replace_all = args["replace_all"].as_bool().unwrap_or(false);
                let delete = args["delete"].as_bool().unwrap_or(false);
                if old.is_empty() {
                    return Err(ToolOutput::error("`old_text` must not be empty."));
                }
                // An empty replacement is a common slip that deletes working code.
                if new.is_empty() && !delete {
                    return Err(ToolOutput::error(
                        "`new_text` is empty, which would delete `old_text`. Give the \
                         replacement text, or set `delete` to true to remove it.",
                    ));
                }
                if delete && !new.is_empty() {
                    return Err(ToolOutput::error(
                        "`delete` is true but `new_text` is not empty: use one or the other.",
                    ));
                }
                let text = read_text(&path)?;
                let mut count = text.matches(old.as_str()).count();
                // Models write `\n`; Windows files often use `\r\n`.
                if count == 0 && text.contains("\r\n") && old.contains('\n') && !old.contains('\r')
                {
                    old = old.replace('\n', "\r\n");
                    new = new.replace('\n', "\r\n");
                    count = text.matches(old.as_str()).count();
                }
                if count == 0 && !replace_all {
                    // The same lines, but with other trailing spaces or line ends.
                    match loose_match(&text, &old) {
                        Loose::One(range) => {
                            let mut updated = text.clone();
                            updated.replace_range(range, &new);
                            std::fs::write(&path, updated).map_err(|e| {
                                ToolOutput::error(format!("Cannot write the file: {e}"))
                            })?;
                            let shown = display(root, &path);
                            return Ok(ToolOutput {
                                touched: vec![shown.clone()],
                                ..ToolOutput::text(format!(
                                    "Edited {shown} (1 replacement; the lines matched apart \
                                     from trailing spaces or line ends)."
                                ))
                            });
                        }
                        Loose::Many(n) => {
                            return Err(ToolOutput::error(format!(
                                "`old_text` appears {n} times (ignoring trailing spaces). \
                                 Include more surrounding lines so it is unique."
                            )));
                        }
                        Loose::None => {}
                    }
                }
                match count {
                    0 => {
                        return Err(ToolOutput::error(not_found_hint(&text, &old)));
                    }
                    n if n > 1 && !replace_all => {
                        return Err(ToolOutput::error(format!(
                            "`old_text` appears {n} times. Include more surrounding lines so it is \
                             unique, or set replace_all."
                        )));
                    }
                    _ => {}
                }
                let updated = if replace_all {
                    text.replace(old.as_str(), &new)
                } else {
                    text.replacen(old.as_str(), &new, 1)
                };
                std::fs::write(&path, updated)
                    .map_err(|e| ToolOutput::error(format!("Cannot write the file: {e}")))?;
                let shown = display(root, &path);
                let n = if replace_all { count } else { 1 };
                Ok(ToolOutput {
                    touched: vec![shown.clone()],
                    ..ToolOutput::text(format!("Edited {shown} ({n} replacement(s))."))
                })
            };
            run().unwrap_or_else(|e| e)
        })
    }
}

/// Where `old` is in `text` when lines are compared without their trailing
/// spaces and line ends (indentation still counts).
enum Loose {
    None,
    /// The byte range to replace.
    One(std::ops::Range<usize>),
    Many(usize),
}

fn loose_match(text: &str, old: &str) -> Loose {
    let wanted: Vec<&str> = old.lines().map(str::trim_end).collect();
    if wanted.iter().all(|l| l.is_empty()) {
        return Loose::None;
    }
    // Each line of the file: where it starts, where its text ends (before the
    // line end), and its text without trailing spaces.
    let mut lines = Vec::new();
    let mut at = 0;
    for raw in text.split_inclusive('\n') {
        let body = raw.trim_end_matches(['\n', '\r']);
        lines.push((at, at + body.len(), body.trim_end()));
        at += raw.len();
    }
    let found: Vec<usize> = (0..lines.len().saturating_sub(wanted.len() - 1))
        .filter(|&i| {
            lines[i..i + wanted.len()]
                .iter()
                .zip(&wanted)
                .all(|(l, w)| l.2 == *w)
        })
        .collect();
    match found.as_slice() {
        [] => Loose::None,
        [i] => {
            let start = lines[*i].0;
            let last = lines[i + wanted.len() - 1];
            // `old_text` ending in a line end replaces that line end too.
            let end = if old.ends_with('\n') {
                lines
                    .get(i + wanted.len())
                    .map_or(text.len(), |next| next.0)
            } else {
                last.1
            };
            Loose::One(start..end)
        }
        many => Loose::Many(many.len()),
    }
}

/// Why `old` was not found, as far as can be seen: where its first line is,
/// and where the text starts to differ.
fn not_found_hint(text: &str, old: &str) -> String {
    const BASE: &str = "`old_text` was not found.";
    let first = old.lines().map(str::trim).find(|l| !l.is_empty());
    let Some(first) = first else {
        return format!("{BASE} Read the file again and copy the exact text.");
    };
    let file: Vec<&str> = text.lines().collect();
    let Some(n) = file.iter().position(|l| l.trim() == first) else {
        return format!(
            "{BASE} Not even its first line is in the file. Read the file again and copy \
             the exact text."
        );
    };
    let wanted: Vec<&str> = old.lines().skip_while(|l| l.trim().is_empty()).collect();
    let differs = wanted
        .iter()
        .zip(&file[n..])
        .position(|(w, f)| w.trim_end() != f.trim_end())
        .unwrap_or(wanted.len().min(file.len() - n));
    format!(
        "{BASE} Its first line is line {} of the file, but line {} differs from it (check \
         indentation, and look-alike characters such as − and -, or “ and \"). Read those \
         lines again and copy them exactly, or replace fewer lines.",
        n + 1,
        n + 1 + differs
    )
}

pub struct ListFiles;

const MAX_LIST_ENTRIES: usize = 500;

impl Tool for ListFiles {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "list_files".into(),
            description:
                "List files and folders in the project (respects .gitignore). Folders end with `/`."
                    .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Folder to list. Default: the project root." },
                    "depth": { "type": "integer", "description": "How many levels deep. Default 3." },
                },
            }),
        }
    }

    fn call<'a>(&'a self, args: Value, ctx: &'a ToolContext) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            let run = || -> Result<ToolOutput, ToolOutput> {
                let root = project_root(ctx)?;
                let dir = resolve_read(
                    root,
                    &ctx.reference_dirs,
                    args["path"].as_str().unwrap_or("."),
                )
                .map_err(ToolOutput::error)?;
                if !dir.is_dir() {
                    return Err(ToolOutput::error("That is not a folder."));
                }
                let depth = args["depth"]
                    .as_u64()
                    .map_or(3, |d| d.clamp(1, 10) as usize);
                let mut entries: Vec<String> = walker(&dir)
                    .max_depth(Some(depth))
                    .build()
                    .filter_map(Result::ok)
                    .filter(|e| e.depth() > 0)
                    .map(|e| {
                        let shown = display(root, e.path());
                        if e.file_type().is_some_and(|t| t.is_dir()) {
                            format!("{shown}/")
                        } else {
                            shown
                        }
                    })
                    .collect();
                entries.sort();
                let total = entries.len();
                entries.truncate(MAX_LIST_ENTRIES);
                let mut out = if entries.is_empty() {
                    "(empty)".to_owned()
                } else {
                    entries.join("\n")
                };
                if total > MAX_LIST_ENTRIES {
                    out.push_str(&format!(
                        "\n… and {} more. List a subfolder to see them.",
                        total - MAX_LIST_ENTRIES
                    ));
                }
                Ok(ToolOutput::text(out))
            };
            run().unwrap_or_else(|e| e)
        })
    }
}

/// .gitignore-aware walker (even without a git repo) that skips build/VCS folders.
pub(crate) fn walker(dir: &std::path::Path) -> ignore::WalkBuilder {
    let mut b = ignore::WalkBuilder::new(dir);
    b.hidden(false)
        .require_git(false)
        .filter_entry(|e| !SKIP_DIRS.contains(&e.file_name().to_string_lossy().as_ref()));
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(dir: &tempfile::TempDir) -> ToolContext {
        ToolContext::for_project(dir.path())
    }

    fn text(out: &ToolOutput) -> String {
        match &out.content[0] {
            ak_types::chat::ContentPart::Text { text } => text.clone(),
            _ => panic!(),
        }
    }

    #[tokio::test]
    async fn write_then_read_with_paging() {
        let dir = tempfile::tempdir().unwrap();
        let c = ctx(&dir);
        let body: String = (1..=10).map(|i| format!("line {i}\n")).collect();
        let w = WriteFile
            .call(json!({ "path": "src/new/file.txt", "content": body }), &c)
            .await;
        assert!(!w.is_error, "{}", text(&w));
        assert_eq!(w.touched, ["src/new/file.txt"]);

        let r = ReadFile
            .call(json!({ "path": "src/new/file.txt" }), &c)
            .await;
        assert!(text(&r).starts_with("line 1\nline 2"));
        let page = ReadFile
            .call(
                json!({ "path": "src/new/file.txt", "offset": 4, "limit": 2 }),
                &c,
            )
            .await;
        assert!(text(&page).starts_with("line 4\nline 5\n"));
        assert!(text(&page).contains("(lines 4–5 of 10"));
    }

    #[tokio::test]
    async fn read_errors_are_friendly() {
        let dir = tempfile::tempdir().unwrap();
        let c = ctx(&dir);
        std::fs::write(dir.path().join("img.png"), [0x89, 0x50, 0xff, 0x00]).unwrap();
        assert!(
            text(&ReadFile.call(json!({ "path": "img.png" }), &c).await)
                .contains("not a text file")
        );
        assert!(
            ReadFile
                .call(json!({ "path": "missing.rs" }), &c)
                .await
                .is_error
        );
        assert!(
            ReadFile
                .call(json!({ "path": "../outside" }), &c)
                .await
                .is_error
        );
        assert!(ReadFile.call(json!({}), &c).await.is_error);
        let none = ToolContext {
            project_dir: None,
            ..ToolContext::for_project(".")
        };
        assert!(text(&ReadFile.call(json!({ "path": "a" }), &none).await).contains("No project"));
    }

    #[tokio::test]
    async fn edit_exact_unique_and_replace_all() {
        let dir = tempfile::tempdir().unwrap();
        let c = ctx(&dir);
        std::fs::write(dir.path().join("a.css"), "red red\nblue").unwrap();
        let dup = EditFile
            .call(
                json!({ "path": "a.css", "old_text": "red", "new_text": "green" }),
                &c,
            )
            .await;
        assert!(dup.is_error && text(&dup).contains("2 times"));
        let all = EditFile
            .call(json!({ "path": "a.css", "old_text": "red", "new_text": "green", "replace_all": true }), &c)
            .await;
        assert!(!all.is_error);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.css")).unwrap(),
            "green green\nblue"
        );
        let miss = EditFile
            .call(
                json!({ "path": "a.css", "old_text": "purple", "new_text": "x" }),
                &c,
            )
            .await;
        assert!(text(&miss).contains("not found"));
        assert_eq!(all.touched, ["a.css"]);
    }

    #[tokio::test]
    async fn lines_match_without_trailing_spaces_and_misses_say_where() {
        let dir = tempfile::tempdir().unwrap();
        let c = ctx(&dir);
        let file = dir.path().join("m.rs");
        // Mixed line ends and trailing spaces, as hand-edited files have.
        std::fs::write(
            &file,
            "fn a() {  \r\n    let x = 1;\n    x\r\n}\r\nfn b() {}\n",
        )
        .unwrap();
        let out = EditFile
            .call(
                json!({
                    "path": "m.rs",
                    "old_text": "fn a() {\n    let x = 1;\n    x\n}",
                    "new_text": "fn a() -> i32 {\n    2\n}",
                }),
                &c,
            )
            .await;
        assert!(!out.is_error, "{}", text(&out));
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "fn a() -> i32 {\r\n    2\r\n}\r\nfn b() {}\n",
            "only those lines (with the file's line ends), the rest as it was"
        );

        // Indentation still counts, and a miss names the line that differs.
        let miss = EditFile
            .call(
                json!({
                    "path": "m.rs",
                    "old_text": "fn a() -> i32 {\n    3\n}",
                    "new_text": "x",
                }),
                &c,
            )
            .await;
        assert!(miss.is_error);
        let hint = text(&miss);
        assert!(
            hint.contains("line 1 of the file") && hint.contains("line 2 differs"),
            "{hint}"
        );
        let nowhere = EditFile
            .call(
                json!({ "path": "m.rs", "old_text": "fn c() {}", "new_text": "x" }),
                &c,
            )
            .await;
        assert!(text(&nowhere).contains("Not even its first line"));
    }

    #[tokio::test]
    async fn an_empty_replacement_needs_delete() {
        let dir = tempfile::tempdir().unwrap();
        let c = ctx(&dir);
        std::fs::write(dir.path().join("a.rs"), "fn a() {}\nfn b() {}\n").unwrap();
        let read = || std::fs::read_to_string(dir.path().join("a.rs")).unwrap();
        let slip = EditFile
            .call(
                json!({ "path": "a.rs", "old_text": "fn b() {}\n", "new_text": "" }),
                &c,
            )
            .await;
        assert!(slip.is_error && text(&slip).contains("delete"));
        assert_eq!(read(), "fn a() {}\nfn b() {}\n", "nothing removed");
        let both = EditFile
            .call(json!({ "path": "a.rs", "old_text": "fn b() {}\n", "new_text": "x", "delete": true }), &c)
            .await;
        assert!(both.is_error);
        let gone = EditFile
            .call(json!({ "path": "a.rs", "old_text": "fn b() {}\n", "new_text": "", "delete": true }), &c)
            .await;
        assert!(!gone.is_error, "{}", text(&gone));
        assert_eq!(read(), "fn a() {}\n");
    }

    #[tokio::test]
    async fn edit_matches_crlf_files() {
        let dir = tempfile::tempdir().unwrap();
        let c = ctx(&dir);
        std::fs::write(dir.path().join("w.rs"), "fn a() {\r\n    1\r\n}\r\n").unwrap();
        let out = EditFile
            .call(json!({ "path": "w.rs", "old_text": "fn a() {\n    1\n}", "new_text": "fn a() {\n    2\n}" }), &c)
            .await;
        assert!(!out.is_error, "{}", text(&out));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("w.rs")).unwrap(),
            "fn a() {\r\n    2\r\n}\r\n"
        );
    }

    #[tokio::test]
    async fn list_respects_gitignore_and_skips_build_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let c = ctx(&dir);
        for f in [
            "src/app.rs",
            "target/debug/x",
            ".git/HEAD",
            "secret.log",
            ".agentknife/PROJECT.md",
        ] {
            let p = dir.path().join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "x").unwrap();
        }
        std::fs::write(dir.path().join(".gitignore"), "*.log\n").unwrap();
        let out = text(&ListFiles.call(json!({}), &c).await);
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines.contains(&"src/") && lines.contains(&"src/app.rs"));
        assert!(lines.contains(&".agentknife/PROJECT.md"));
        assert!(!out.contains("target") && !out.contains(".git/") && !out.contains("secret.log"));
        let sub = text(&ListFiles.call(json!({ "path": "src" }), &c).await);
        assert_eq!(sub, "src/app.rs");
    }
}
