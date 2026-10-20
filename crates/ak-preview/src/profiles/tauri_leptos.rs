//! The `tauri-leptos` stack: Tauri 2 backend, Leptos CSR screens built by Trunk,
//! Tailwind styles, ak-ui components and the ak-bridge preview bridge.
//!
//! This is the only place that knows those names (checked by `cargo xtask ci`).

use ak_types::stack::{
    Artifact, BundlePlan, CommandSpec, HostOs, KnowledgePack, PreviewPlan, ReadySignal,
    StackProfile, Template, TemplateFile, TemplateValue, Tool,
};

/// `templates/tauri-leptos`, embedded by build.rs.
static FILES: &[TemplateFile] = include!(concat!(env!("OUT_DIR"), "/tauri_leptos_files.rs"));

// `knowledge/tauri-leptos` (KNOWLEDGE_ID, KNOWLEDGE_VERSION, KNOWLEDGE_TOPICS),
// embedded by build.rs.
include!(concat!(env!("OUT_DIR"), "/tauri_leptos_knowledge.rs"));

/// Literal defaults in the template (same as its template.toml [replace]).
const REPLACE: &[(TemplateValue, &str)] = &[
    (TemplateValue::ProductName, "My App"),
    (TemplateValue::Identifier, "com.agentknife.myapp"),
];

/// ak-bridge's preview settings (`ak_bridge::protocol`; a test keeps them equal).
pub(crate) const BRIDGE_READY_PREFIX: &str = "AK_BRIDGE_READY ";
pub(crate) const BRIDGE_URL_PARAM: &str = "ak_bridge";
pub(crate) const BRIDGE_ENV: [&str; 3] = ["AK_BRIDGE_PORT", "AK_BRIDGE_TOKEN", "AK_BRIDGE_ORIGIN"];

/// Every doctor item this stack needs (those of other OSes are not reported
/// there, so listing them is harmless).
const DOCTOR: &[&str] = &[
    "rust",
    "wasm32-target",
    "trunk",
    "wasm-bindgen",
    "tauri-cli",
    "tailwind",
    "msvc-build-tools",
    "windows-sdk",
    "webview2",
    "xcode-clt",
    "linux-webkit2gtk",
    "linux-javascriptcore",
    "linux-libsoup",
    "linux-appindicator",
    "linux-librsvg",
    "linux-c-compiler",
];

const ARTIFACTS: &[Artifact] = &[
    Artifact {
        os: HostOs::Windows,
        kind: "nsis",
        dir: "target/release/bundle/nsis",
        extension: "exe",
    },
    Artifact {
        os: HostOs::MacOs,
        kind: "dmg",
        dir: "target/release/bundle/dmg",
        extension: "dmg",
    },
    Artifact {
        os: HostOs::Linux,
        kind: "appimage",
        dir: "target/release/bundle/appimage",
        extension: "AppImage",
    },
    Artifact {
        os: HostOs::Linux,
        kind: "deb",
        dir: "target/release/bundle/deb",
        extension: "deb",
    },
];

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

/// Trunk exits on `NO_COLOR=1` (it expects true/false) and checks the network
/// for updates unless told not to.
fn trunk_env() -> (Vec<(String, String)>, Vec<String>) {
    (
        vec![("TRUNK_SKIP_VERSION_CHECK".into(), "true".into())],
        vec!["NO_COLOR".into()],
    )
}

pub struct TauriLeptos;

impl StackProfile for TauriLeptos {
    fn id(&self) -> &'static str {
        "tauri-leptos"
    }

    fn template(&self) -> Template {
        Template {
            files: FILES,
            replace: REPLACE,
            renames: &[("gitignore", ".gitignore")],
        }
    }

    fn knowledge_pack(&self) -> KnowledgePack {
        KnowledgePack {
            id: KNOWLEDGE_ID,
            version: KNOWLEDGE_VERSION,
            topics: KNOWLEDGE_TOPICS,
        }
    }

    fn doctor_requirements(&self) -> &'static [&'static str] {
        DOCTOR
    }

    fn preview(&self) -> PreviewPlan {
        let (env, env_remove) = trunk_env();
        PreviewPlan {
            frontend: CommandSpec {
                tool: Tool::Trunk,
                args: strings(&[
                    "serve",
                    "--features",
                    "dev",
                    "--address",
                    "127.0.0.1",
                    "--port",
                    "{port}",
                ]),
                cwd: "",
                env,
                env_remove: env_remove.clone(),
            },
            frontend_ready: ReadySignal::HttpOk,
            backend: Some(CommandSpec {
                tool: Tool::Cargo,
                args: strings(&[
                    "run",
                    "--manifest-path",
                    "src-tauri/Cargo.toml",
                    "--features",
                    "dev",
                ]),
                cwd: "",
                env: vec![
                    (BRIDGE_ENV[0].into(), "{bridge_port}".into()),
                    (BRIDGE_ENV[1].into(), "{bridge_token}".into()),
                    (BRIDGE_ENV[2].into(), "{bridge_origin}".into()),
                ],
                env_remove,
            }),
            backend_ready: ReadySignal::StdoutPrefix(BRIDGE_READY_PREFIX),
            backend_watch: &["src-tauri", "ak/ak-bridge"],
            bridge_url_param: BRIDGE_URL_PARAM,
        }
    }

    fn bundle(&self) -> BundlePlan {
        let (env, env_remove) = trunk_env();
        BundlePlan {
            // tauri.conf.json runs `trunk build --release` first (no `dev`
            // feature: the bridge is not in the installer).
            command: CommandSpec {
                tool: Tool::CargoTauri,
                args: strings(&["build"]),
                cwd: "",
                env,
                env_remove,
            },
            artifacts: ARTIFACTS,
        }
    }
}

#[cfg(test)]
mod tests {
    use ak_types::stack::ProjectValues;

    use super::*;

    fn file(path: &str) -> Option<&'static TemplateFile> {
        FILES.iter().find(|f| f.path == path)
    }

    #[test]
    fn the_whole_template_is_embedded_and_nothing_else() {
        for path in [
            "Cargo.toml",
            "Cargo.lock",
            "Trunk.toml",
            "index.html",
            "gitignore",
            "style/input.css",
            "src/main.rs",
            "src-tauri/Cargo.toml",
            "src-tauri/build.rs",
            "src-tauri/tauri.conf.json",
            "src-tauri/capabilities/default.json",
            "src-tauri/src/main.rs",
            "src-tauri/icons/icon.ico",
            "ak/ak-bridge/Cargo.toml",
            "ak/ak-ui/src/lib.rs",
        ] {
            assert!(file(path).is_some(), "{path} is missing");
        }
        for f in FILES {
            assert!(
                !f.path.starts_with("target/")
                    && !f.path.starts_with("dist/")
                    && f.path != "template.toml"
                    && !f.path.contains('\\'),
                "{} must not be in a project",
                f.path
            );
        }
    }

    #[test]
    fn replacements_match_template_toml_and_the_files() {
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../templates/tauri-leptos/template.toml"
        ))
        .unwrap();
        let t: toml::Table = toml::from_str(&text).unwrap();
        let replace = t["replace"].as_table().unwrap();
        assert_eq!(replace["product_name"].as_str(), Some(REPLACE[0].1));
        assert_eq!(replace["identifier"].as_str(), Some(REPLACE[1].1));

        let out = TauriLeptos.template().instantiate(&ProjectValues {
            product_name: "Todo List".into(),
            identifier: "com.agentknife.todolist".into(),
        });
        let get = |p: &str| {
            out.iter()
                .find(|(path, _)| path == p)
                .map(|(_, c)| String::from_utf8_lossy(c).into_owned())
                .unwrap()
        };
        let conf = get("src-tauri/tauri.conf.json");
        assert!(conf.contains(r#""productName": "Todo List""#), "{conf}");
        assert!(conf.contains(r#""identifier": "com.agentknife.todolist""#));
        assert!(conf.contains(r#""title": "Todo List""#));
        assert!(get("index.html").contains("<title>Todo List</title>"));
        assert!(get(".gitignore").contains("/target/"));
        // No default left anywhere in the project's text.
        for (path, contents) in &out {
            let text = String::from_utf8_lossy(contents);
            assert!(!text.contains("com.agentknife.myapp"), "{path}");
            assert!(!text.contains("My App"), "{path} still says My App");
        }
    }

    #[test]
    fn knowledge_is_embedded() {
        let pack = TauriLeptos.knowledge_pack();
        assert_eq!(pack.id, TauriLeptos.id());
        assert!(pack.topics.len() >= 10);
        for id in [
            "overview",
            "ak-ui",
            "ak-bridge",
            "data-ak-id",
            "example-todo",
        ] {
            let topic = pack.topic(id).unwrap_or_else(|| panic!("{id} missing"));
            assert!(topic.body.starts_with("# "), "{id}");
            assert!(!topic.summary.is_empty());
        }
        // The worked example follows the data-ak-id convention.
        assert!(
            pack.topic("example-todo")
                .unwrap()
                .body
                .contains("data-ak-id")
        );
    }

    #[test]
    fn bridge_settings_match_ak_bridge() {
        use ak_bridge::protocol as p;
        assert_eq!(BRIDGE_READY_PREFIX, p::READY_PREFIX);
        assert_eq!(BRIDGE_URL_PARAM, p::URL_PARAM);
        assert_eq!(BRIDGE_ENV, [p::ENV_PORT, p::ENV_TOKEN, p::ENV_ORIGIN]);
    }

    #[test]
    fn preview_and_bundle_never_mix_up_the_dev_feature() {
        let preview = TauriLeptos.preview();
        assert!(
            preview
                .frontend
                .args
                .windows(2)
                .any(|w| w == ["--features", "dev"])
        );
        let backend = preview.backend.unwrap();
        assert!(backend.args.windows(2).any(|w| w == ["--features", "dev"]));
        assert!(backend.env.iter().any(|(_, v)| v == "{bridge_token}"));
        assert!(preview.frontend.env_remove.contains(&"NO_COLOR".to_owned()));
        let bundle = TauriLeptos.bundle();
        assert!(!bundle.command.args.iter().any(|a| a.contains("dev")));
        // The installer build must not enable `dev` behind our back either.
        let conf = String::from_utf8_lossy(file("src-tauri/tauri.conf.json").unwrap().contents);
        assert!(conf.contains(r#""beforeBuildCommand": "trunk build --release""#));
    }
}
