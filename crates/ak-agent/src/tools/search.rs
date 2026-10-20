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
                          `path:line: text` for each matching line."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Regular expression (Rust regex syntax)" },
                    "path": { "type": "string", "description": "Folder to search. Default: the project root." },
                    "glob": { "type": "string", "description": "Only files matching this glob, e.g. *.rs" },
                    "case_insensitive": { "type": "boolean" },
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

                let mut hits = Vec::new();
                let mut truncated = false;
                'files: for file in files {
                    // Binary or non-UTF-8 files are skipped.
                    let Ok(text) = std::fs::read_to_string(&file) else {
                        continue;
                    };
                    for (n, line) in text.lines().enumerate() {
                        if re.is_match(line) {
                            if hits.len() == MAX_MATCHES {
                                truncated = true;
                                break 'files;
                            }
                            let shown: String = line.trim().chars().take(MAX_LINE_CHARS).collect();
                            hits.push(format!("{}:{}: {shown}", display(root, &file), n + 1));
                        }
                    }
                }
                let mut out = if hits.is_empty() {
                    "No matches.".to_owned()
                } else {
                    hits.join("\n")
                };
                if truncated {
                    out.push_str(&format!(
                        "\n(stopped after {MAX_MATCHES} matches; narrow the search)"
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

    fn text(out: &ToolOutput) -> String {
        match &out.content[0] {
            ak_types::chat::ContentPart::Text { text } => text.clone(),
            _ => panic!(),
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
            "No matches."
        );
        let bad = SearchFiles.call(json!({ "pattern": "(" }), &c).await;
        assert!(bad.is_error && text(&bad).contains("Invalid pattern"));
    }
}
