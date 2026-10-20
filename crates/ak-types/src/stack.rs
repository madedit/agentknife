//! What AgentKnife needs to know about an app stack (AGENTS.md: StackProfile).
//!
//! Everything stack-specific lives behind [`StackProfile`]: the project template,
//! the knowledge pack, which tools the doctor must have ready, how to run the
//! live preview and how to build installers. The core crates work with these
//! plain descriptions and never name a stack.

use crate::doctor::{CheckStatus, DoctorReport};

/// An app stack AgentKnife can build with.
pub trait StackProfile: Send + Sync {
    /// Stable id, stored in each project (`project.json`).
    fn id(&self) -> &'static str;
    fn template(&self) -> Template;
    fn knowledge_pack(&self) -> KnowledgePack;
    /// Doctor item ids (`doctor-item-<id>`) that must be Ok to build. Ids the
    /// doctor does not report on this OS are ignored.
    fn doctor_requirements(&self) -> &'static [&'static str];
    fn preview(&self) -> PreviewPlan;
    fn bundle(&self) -> BundlePlan;
}

impl DoctorReport {
    /// Everything `required` that the report lists is Ok.
    pub fn ready_for(&self, required: &[&str]) -> bool {
        self.items
            .iter()
            .filter(|i| required.contains(&i.id.as_str()))
            .all(|i| i.status == CheckStatus::Ok)
    }
}

// ---- Template ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemplateFile {
    /// Relative, with `/` separators.
    pub path: &'static str,
    pub contents: &'static [u8],
}

/// A value each project gets its own copy of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateValue {
    /// The app's display name (window title, installer name).
    ProductName,
    /// Reverse-DNS app id, e.g. `com.agentknife.todolist`.
    Identifier,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Template {
    pub files: &'static [TemplateFile],
    /// Literal text in the template, replaced by the project's value.
    pub replace: &'static [(TemplateValue, &'static str)],
    /// Files stored under another name (`gitignore` -> `.gitignore`: a dotfile in
    /// AgentKnife's own repo would apply to the repo itself).
    pub renames: &'static [(&'static str, &'static str)],
}

/// A project's values for [`TemplateValue`]s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectValues {
    pub product_name: String,
    pub identifier: String,
}

impl ProjectValues {
    /// Values for an app named `name`. `unique` (e.g. part of the project id)
    /// keeps the identifier valid when the name has no ASCII letters.
    pub fn for_name(name: &str, unique: &str) -> ProjectValues {
        let product_name = name.trim().to_owned();
        let mut slug: String = product_name
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .map(|c| c.to_ascii_lowercase())
            .collect();
        if !slug.starts_with(|c: char| c.is_ascii_lowercase()) {
            let tail: String = unique
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .map(|c| c.to_ascii_lowercase())
                .collect();
            slug = format!("app{slug}{tail}");
        }
        ProjectValues {
            product_name,
            identifier: format!("com.agentknife.{slug}"),
        }
    }

    fn get(&self, v: TemplateValue) -> &str {
        match v {
            TemplateValue::ProductName => &self.product_name,
            TemplateValue::Identifier => &self.identifier,
        }
    }
}

/// Text formats where the replacements apply; anything else is copied as is.
const TEXT_EXTENSIONS: &[&str] = &["rs", "toml", "json", "html", "css", "md", "svg"];

fn escape_for(path: &str, value: &str) -> String {
    // Product names land inside JSON and TOML strings.
    if path.ends_with(".json") || path.ends_with(".toml") || path.ends_with(".rs") {
        value.replace('\\', "\\\\").replace('"', "\\\"")
    } else if path.ends_with(".html") || path.ends_with(".svg") {
        value
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    } else {
        value.to_owned()
    }
}

impl Template {
    /// The project's files: renamed, with the project's values filled in.
    pub fn instantiate(&self, values: &ProjectValues) -> Vec<(String, Vec<u8>)> {
        self.files
            .iter()
            .map(|f| {
                let path = self
                    .renames
                    .iter()
                    .find(|(from, _)| *from == f.path)
                    .map_or(f.path, |(_, to)| *to)
                    .to_owned();
                let is_text = f
                    .path
                    .rsplit_once('.')
                    .is_some_and(|(_, ext)| TEXT_EXTENSIONS.contains(&ext));
                let contents = match std::str::from_utf8(f.contents) {
                    Ok(text) if is_text => {
                        let mut text = text.to_owned();
                        for (value, default) in self.replace {
                            text = text.replace(default, &escape_for(f.path, values.get(*value)));
                        }
                        text.into_bytes()
                    }
                    _ => f.contents.to_vec(),
                };
                (path, contents)
            })
            .collect()
    }
}

// ---- Knowledge ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnowledgePack {
    /// Folder under `knowledge/` (P1-KNOW-001).
    pub id: &'static str,
    pub version: &'static str,
}

// ---- Commands ----

/// A program from AgentKnife's toolchain (run by full path, with its env).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Cargo,
    Trunk,
    /// The `cargo-tauri` binary.
    CargoTauri,
}

/// A command in the project. `args` and env values may contain placeholders:
/// `{port}` (frontend dev server), `{bridge_port}`, `{bridge_token}`,
/// `{bridge_origin}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub tool: Tool,
    pub args: Vec<String>,
    /// Working directory, relative to the project root (`""` = the root).
    pub cwd: &'static str,
    pub env: Vec<(String, String)>,
    /// Inherited variables to remove (some break the tool).
    pub env_remove: Vec<String>,
}

/// Fills `{name}` placeholders.
pub fn fill(text: &str, values: &[(&str, &str)]) -> String {
    values.iter().fold(text.to_owned(), |t, (k, v)| {
        t.replace(&format!("{{{k}}}"), v)
    })
}

/// How to tell a started process is ready.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadySignal {
    /// The frontend URL answers HTTP 200.
    HttpOk,
    /// A stdout line starting with this (the rest may carry JSON).
    StdoutPrefix(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewPlan {
    /// The dev server for the screens, serving on `{port}`.
    pub frontend: CommandSpec,
    pub frontend_ready: ReadySignal,
    /// The app's backend serving its commands to the preview, if the stack has one.
    pub backend: Option<CommandSpec>,
    pub backend_ready: ReadySignal,
    /// Changes under these project paths need a backend restart (the frontend
    /// server rebuilds by itself).
    pub backend_watch: &'static [&'static str],
    /// URL query parameter that tells the page where the bridge is.
    pub bridge_url_param: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostOs {
    Windows,
    MacOs,
    Linux,
}

/// An installer the bundle step produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Artifact {
    pub os: HostOs,
    /// Short kind for messages (`nsis`, `dmg`, `appimage`, `deb`).
    pub kind: &'static str,
    /// Folder (relative to the project) and file extension where it appears.
    pub dir: &'static str,
    pub extension: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundlePlan {
    pub command: CommandSpec,
    pub artifacts: &'static [Artifact],
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doctor::{CheckItem, CheckScope};

    #[test]
    fn identifiers_are_valid_reverse_dns() {
        assert_eq!(
            ProjectValues::for_name(" Todo List! ", "p_1").identifier,
            "com.agentknife.todolist"
        );
        // Names without ASCII letters still get a valid id.
        assert_eq!(
            ProjectValues::for_name("待辦清單", "project_8f3a").identifier,
            "com.agentknife.appproject8f3a"
        );
        assert_eq!(
            ProjectValues::for_name("2048", "x9").identifier,
            "com.agentknife.app2048x9"
        );
    }

    #[test]
    fn instantiate_renames_and_fills_values() {
        static FILES: &[TemplateFile] = &[
            TemplateFile {
                path: "src-tauri/tauri.conf.json",
                contents: br#"{"productName": "My App", "identifier": "com.agentknife.myapp"}"#,
            },
            TemplateFile {
                path: "index.html",
                contents: b"<title>My App</title>",
            },
            TemplateFile {
                path: "gitignore",
                contents: b"/target/",
            },
            TemplateFile {
                path: "icons/icon.png",
                contents: b"\x89PNG My App",
            },
        ];
        let t = Template {
            files: FILES,
            replace: &[
                (TemplateValue::ProductName, "My App"),
                (TemplateValue::Identifier, "com.agentknife.myapp"),
            ],
            renames: &[("gitignore", ".gitignore")],
        };
        let out = t.instantiate(&ProjectValues {
            product_name: "Tom's \"Best\" <List>".into(),
            identifier: "com.agentknife.toms".into(),
        });
        let get = |p: &str| {
            out.iter()
                .find(|(path, _)| path == p)
                .map(|(_, c)| String::from_utf8_lossy(c).into_owned())
                .unwrap()
        };
        assert_eq!(
            get("src-tauri/tauri.conf.json"),
            r#"{"productName": "Tom's \"Best\" <List>", "identifier": "com.agentknife.toms"}"#
        );
        assert_eq!(
            get("index.html"),
            "<title>Tom's \"Best\" &lt;List&gt;</title>"
        );
        assert_eq!(get(".gitignore"), "/target/");
        // Binary files are never touched.
        assert_eq!(get("icons/icon.png"), "\u{fffd}PNG My App");
    }

    #[test]
    fn placeholders_and_readiness() {
        assert_eq!(
            fill("--port {port} {port}", &[("port", "5173")]),
            "--port 5173 5173"
        );
        let item = |id: &str, status| CheckItem {
            id: id.into(),
            scope: CheckScope::Isolated,
            status,
            found: None,
            wanted: None,
            detail: None,
        };
        let report = DoctorReport {
            items: vec![
                item("trunk", CheckStatus::Ok),
                item("webview2", CheckStatus::Ok),
                item("unrelated", CheckStatus::Missing),
            ],
        };
        // xcode-clt is not reported on this OS: ignored.
        assert!(report.ready_for(&["trunk", "webview2", "xcode-clt"]));
        assert!(!report.ready_for(&["unrelated"]));
    }
}
