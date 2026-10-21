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
                          (copy it from read_file) and appear once, unless `replace_all` is true."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Project-relative path" },
                    "old_text": { "type": "string", "description": "Exact text to replace, with enough surrounding lines to be unique" },
                    "new_text": { "type": "string", "description": "Replacement text" },
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
                if old.is_empty() {
                    return Err(ToolOutput::error("`old_text` must not be empty."));
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
                match count {
                    0 => {
                        return Err(ToolOutput::error(
                            "`old_text` was not found. Read the file again and copy the exact text.",
                        ));
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
