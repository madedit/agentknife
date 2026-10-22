//! `add_library`: copies a pinned JS library that ships with AgentKnife into the
//! project (nothing is downloaded; the files are too big for `write_file`).

use ak_types::chat::ToolSpec;
use ak_types::stack::KnowledgePack;
use futures_util::future::BoxFuture;
use serde_json::{Value, json};

use super::path::{display, resolve};
use super::{project_root, str_arg};
use crate::tool::{Tool, ToolContext, ToolOutput};

pub const ADD_LIBRARY: &str = "add_library";

pub struct AddLibrary {
    pack: KnowledgePack,
}

impl AddLibrary {
    pub fn new(pack: KnowledgePack) -> Self {
        Self { pack }
    }

    fn index(&self) -> String {
        let mut s = String::from("Libraries:\n");
        for l in self.pack.libraries {
            s.push_str(&format!(
                "- {} {} (into {}/): {}\n",
                l.id, l.version, l.target, l.summary
            ));
        }
        s
    }
}

impl Tool for AddLibrary {
    fn changes_project(&self) -> bool {
        true
    }

    fn spec(&self) -> ToolSpec {
        let ids: Vec<&str> = self.pack.libraries.iter().map(|l| l.id).collect();
        ToolSpec {
            name: ADD_LIBRARY.into(),
            description: format!(
                "Add a JavaScript library that ships with AgentKnife to the project, at the \
                 exact version its docs describe. Use this instead of downloading a library, \
                 a CDN link or npm (none of those work here). Adding it again restores the \
                 original files.\n{}",
                self.index()
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "library": {
                        "type": "string",
                        "enum": ids,
                        "description": "Library id from the list above",
                    },
                },
                "required": ["library"],
            }),
        }
    }

    fn call<'a>(&'a self, args: Value, ctx: &'a ToolContext) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            let run = || -> Result<ToolOutput, ToolOutput> {
                let root = project_root(ctx)?;
                let id = str_arg(&args, "library")?.trim();
                let lib = self.pack.library(id).ok_or_else(|| {
                    ToolOutput::error(format!("There is no library `{id}`. {}", self.index()))
                })?;
                let mut touched = Vec::new();
                for f in lib.files {
                    let path = resolve(root, &format!("{}/{}", lib.target, f.path))
                        .map_err(ToolOutput::error)?;
                    if std::fs::read(&path).is_ok_and(|old| old == f.contents) {
                        continue;
                    }
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent).map_err(|e| {
                            ToolOutput::error(format!("Cannot create the folder: {e}"))
                        })?;
                    }
                    std::fs::write(&path, f.contents)
                        .map_err(|e| ToolOutput::error(format!("Cannot write the file: {e}")))?;
                    touched.push(display(root, &path));
                }
                let files: Vec<String> = lib
                    .files
                    .iter()
                    .map(|f| format!("{}/{}", lib.target, f.path))
                    .collect();
                Ok(ToolOutput {
                    touched,
                    ..ToolOutput::text(format!(
                        "{} {} is in {}/ ({}). {}",
                        lib.id,
                        lib.version,
                        lib.target,
                        files.join(", "),
                        lib.summary
                    ))
                })
            };
            run().unwrap_or_else(|e| e)
        })
    }
}

#[cfg(test)]
mod tests {
    use ak_types::stack::{Library, TemplateFile};

    use super::*;

    static LIBS: &[Library] = &[Library {
        id: "three",
        version: "0.1.0",
        summary: "3D. Read topic `three-d`.",
        target: "vendor/three",
        files: &[
            TemplateFile {
                path: "three.module.js",
                contents: b"export {}",
            },
            TemplateFile {
                path: "addons/x.js",
                contents: b"import 'three'",
            },
        ],
    }];

    fn tool() -> AddLibrary {
        AddLibrary::new(KnowledgePack {
            id: "stack",
            version: "1.0.0",
            topics: &[],
            libraries: LIBS,
        })
    }

    #[tokio::test]
    async fn copies_the_files_and_restores_them_when_added_again() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolContext::for_project(dir.path());
        let t = tool();
        assert!(t.changes_project());
        assert_eq!(
            t.spec().input_schema["properties"]["library"]["enum"],
            json!(["three"])
        );

        let out = t.call(json!({ "library": "three" }), &ctx).await;
        assert!(!out.is_error, "{out:?}");
        assert_eq!(
            out.touched,
            ["vendor/three/three.module.js", "vendor/three/addons/x.js"]
        );
        let added = dir.path().join("vendor/three/addons/x.js");
        assert_eq!(std::fs::read(&added).unwrap(), b"import 'three'");

        // Unchanged files are not touched again; a changed one is put back.
        std::fs::write(&added, "broken").unwrap();
        let out = t.call(json!({ "library": "three" }), &ctx).await;
        assert_eq!(out.touched, ["vendor/three/addons/x.js"]);
        assert_eq!(std::fs::read(&added).unwrap(), b"import 'three'");

        let out = t.call(json!({ "library": "jquery" }), &ctx).await;
        assert!(out.is_error);
    }
}
