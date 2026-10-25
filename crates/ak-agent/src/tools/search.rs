//! search_files: regex search over project text files.

use ak_types::chat::ToolSpec;
use futures_util::future::BoxFuture;
use serde_json::{Value, json};

use super::files::walker;
use super::path::{display, resolve_read};
use super::{project_root, str_arg};
use crate::tool::{Tool, ToolContext, ToolOutput};

const MAX_MATCHES: usize = 200;
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_LINE_CHARS: usize = 300;

pub struct SearchFiles;

impl Tool for SearchFiles {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "search_files".into(),
            description: "Search project text files with a regular expression. Returns \
                          `path:line: text` for each matching line (at most 200), or with \
                          `files_only` each matching file once with its number of matches."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Regular expression (Rust regex syntax)" },
                    "path": { "type": "string", "description": "Folder to search. Default: the project root." },
                    "glob": { "type": "string", "description": "Only files matching this glob, e.g. *.rs" },
                    "case_insensitive": { "type": "boolean" },
                    "files_only": { "type": "boolean", "description": "List matching files and how many lines match, not the lines." },
                    "max_per_file": { "type": "integer", "description": "Show at most this many lines per file (the rest are counted)." },
                },
                "required": ["pattern"],
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
                let re = regex::RegexBuilder::new(str_arg(&args, "pattern")?)
                    .case_insensitive(args["case_insensitive"].as_bool().unwrap_or(false))
                    .build()
                    .map_err(|e| ToolOutput::error(format!("Invalid pattern: {e}")))?;
                let mut walk = walker(&dir);
                if let Some(glob) = args["glob"].as_str().filter(|g| !g.is_empty()) {
                    let mut ov = ignore::overrides::OverrideBuilder::new(&dir);
                    ov.add(glob)
                        .map_err(|e| ToolOutput::error(format!("Invalid glob: {e}")))?;
                    walk.overrides(
                        ov.build()
                            .map_err(|e| ToolOutput::error(format!("Invalid glob: {e}")))?,
                    );
                }
                let mut files: Vec<_> = walk
                    .build()
                    .filter_map(Result::ok)
                    .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
                    .filter(|e| e.metadata().is_ok_and(|m| m.len() <= MAX_FILE_BYTES))
                    .map(|e| e.into_path())
                    .collect();
                files.sort();
                let searched = files.len();

                let files_only = args["files_only"].as_bool().unwrap_or(false);
                let per_file = args["max_per_file"]
                    .as_u64()
                    .filter(|n| *n > 0)
                    .map_or(usize::MAX, |n| n as usize);
                let mut hits = Vec::new();
                let mut truncated = false;
                'files: for file in files {
                    // Binary or non-UTF-8 files are skipped.
                    let Ok(text) = std::fs::read_to_string(&file) else {
                        continue;
                    };
                    let name = display(root, &file);
                    let mut found = 0;
                    for (n, line) in text.lines().enumerate() {
                        if !re.is_match(line) {
                            continue;
                        }
                        found += 1;
                        if files_only || found > per_file {
                            continue;
                        }
                        if hits.len() == MAX_MATCHES {
                            truncated = true;
                            break 'files;
                        }
                        let shown: String = line.trim().chars().take(MAX_LINE_CHARS).collect();
                        hits.push(format!("{name}:{}: {shown}", n + 1));
                    }
                    if found == 0 {
                        continue;
                    }
                    if files_only {
                        if hits.len() == MAX_MATCHES {
                            truncated = true;
                            break;
                        }
                        hits.push(format!("{name}: {found} matching lines"));
                    } else if found > per_file {
                        hits.push(format!("{name}: … {} more", found - per_file));
                    }
                }
                // Say what was searched: a glob that matches no file (a guessed
                // file name) looks the same as a pattern that matches no line.
                let mut out = if searched == 0 {
                    "No files to search there. Check the folder and the glob with list_files                      (file names are often not what you would guess)."
                        .to_owned()
                } else if hits.is_empty() {
                    format!(
                        "No matches in {searched} file{}.",
                        if searched == 1 { "" } else { "s" }
                    )
                } else {
                    hits.join("\n")
                };
                if truncated {
                    out.push_str(&format!(
                        "\n(stopped after {MAX_MATCHES} results; narrow the search with `path` or \
                         `glob`, or use `files_only` / `max_per_file`)"
                    ));
                }
                Ok(ToolOutput::text(out))
            };
            run().unwrap_or_else(|e| e)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn files_only_and_max_per_file_keep_results_short() {
        let dir = tempfile::tempdir().unwrap();
        let c = ToolContext::for_project(dir.path());
        std::fs::write(dir.path().join("a.rs"), "x1\nx2\nx3\n").unwrap();
        std::fs::write(dir.path().join("b.rs"), "y\nx4\n").unwrap();
        let files = text(
            &SearchFiles
                .call(json!({ "pattern": "x", "files_only": true }), &c)
                .await,
        );
        assert_eq!(files, "a.rs: 3 matching lines\nb.rs: 1 matching lines");
        let few = text(
            &SearchFiles
                .call(json!({ "pattern": "x", "max_per_file": 1 }), &c)
                .await,
        );
        assert_eq!(few, "a.rs:1: x1\na.rs: … 2 more\nb.rs:2: x4");
    }

    fn text(out: &ToolOutput) -> String {
        match &out.content[0] {
            ak_types::chat::ContentPart::Text { text } => text.clone(),
            _ => panic!(),
        }
    }

    #[tokio::test]
    async fn a_file_as_path_searches_that_file() {
        let dir = tempfile::tempdir().unwrap();
        let c = ToolContext::for_project(dir.path());
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/gen_Canvas.rs"),
            "pub fn fill_text() {}
",
        )
        .unwrap();
        for glob in [None, Some("*.rs"), Some("other.rs")] {
            let mut args = json!({ "pattern": "fill_text", "path": "src/gen_Canvas.rs" });
            if let Some(g) = glob {
                args["glob"] = json!(g);
            }
            let out = text(&SearchFiles.call(args, &c).await);
            assert_eq!(
                out, "src/gen_Canvas.rs:1: pub fn fill_text() {}",
                "glob {glob:?}"
            );
        }
    }

    #[tokio::test]
    async fn finds_lines_with_glob_and_case() {
        let dir = tempfile::tempdir().unwrap();
        let c = ToolContext::for_project(dir.path());
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/app.rs"),
            "fn main() {\n    let Button = 1;\n}\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("style.css"), ".button { color: red }\n").unwrap();
        std::fs::write(dir.path().join("bin.dat"), [0xff, 0xfe, b'b']).unwrap();

        let all = text(
            &SearchFiles
                .call(json!({ "pattern": "button", "case_insensitive": true }), &c)
                .await,
        );
        assert_eq!(
            all,
            "src/app.rs:2: let Button = 1;\nstyle.css:1: .button { color: red }"
        );
        let rs = text(
            &SearchFiles
                .call(json!({ "pattern": "(?i)button", "glob": "*.rs" }), &c)
                .await,
        );
        assert_eq!(rs, "src/app.rs:2: let Button = 1;");
        assert_eq!(
            text(&SearchFiles.call(json!({ "pattern": "zzz" }), &c).await),
            "No matches in 3 files."
        );
        let none = text(
            &SearchFiles
                .call(json!({ "pattern": "x", "glob": "*canvas*" }), &c)
                .await,
        );
        assert!(none.starts_with("No files to search there."), "{none}");
        let bad = SearchFiles.call(json!({ "pattern": "(" }), &c).await;
        assert!(bad.is_error && text(&bad).contains("Invalid pattern"));
    }
}
