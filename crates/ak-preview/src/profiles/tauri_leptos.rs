//! The `tauri-leptos` stack: Tauri 2 backend, Leptos CSR screens built by Trunk,
//! Tailwind styles, ak-ui components and the ak-bridge preview bridge.
//!
//! This is the only place that knows those names (checked by `cargo xtask ci`).

use ak_types::stack::{
    ArchiveFormat, BundleOutput, BundlePlan, BundleTarget, CommandSpec, HostOs, JsonField,
    KnowledgePack, LogMarkers, PreviewMarker, PreviewPlan, ReadySignal, ScanIn, StackProfile,
    Template, TemplateFile, TemplateValue, Tool,
};

/// `templates/tauri-leptos`, embedded by build.rs.
static FILES: &[TemplateFile] = include!(concat!(env!("OUT_DIR"), "/tauri_leptos_files.rs"));

// `knowledge/tauri-leptos` (KNOWLEDGE_ID, KNOWLEDGE_VERSION, KNOWLEDGE_TOPICS, KNOWLEDGE_LIBRARIES),
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

/// Text only the bridge's preview code has (ak_bridge::protocol::READY_PREFIX in
/// the backend, the inspector's overlay id in the screens).
const PREVIEW_MARKERS: &[PreviewMarker] = &[
    PreviewMarker {
        scan: ScanIn::Program,
        text: "AK_BRIDGE_READY",
    },
    PreviewMarker {
        scan: ScanIn::Dir("dist"),
        text: "__ak_inspector_overlay",
    },
];

/// Where the preview's dev server writes the screens. Not `dist/`: a release
/// build writes there, and the preview would then serve screens without `dev`.
const PREVIEW_DIST: &str = "target/ak-preview-dist";

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

/// cargo's progress for the build window (preview builds only).
fn progress_env() -> impl Iterator<Item = (String, String)> {
    crate::process::CARGO_PROGRESS_ENV
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
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
            libraries: KNOWLEDGE_LIBRARIES,
        }
    }

    fn doctor_requirements(&self) -> &'static [&'static str] {
        DOCTOR
    }

    fn preview(&self) -> PreviewPlan {
        let (mut env, env_remove) = trunk_env();
        env.extend(progress_env());
        PreviewPlan {
            frontend: CommandSpec {
                tool: Tool::Trunk,
                args: strings(&[
                    "serve",
                    "--features",
                    "dev",
                    "--dist",
                    PREVIEW_DIST,
                    "--address",
                    "127.0.0.1",
                    "--port",
                    "{port}",
                ]),
                cwd: "",
                env,
                env_remove: env_remove.clone(),
            },
            frontend_log: LogMarkers {
                building: &["INFO starting build"],
                success: &["INFO success"],
                error: &["ERROR error"],
            },
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
                env: [
                    (BRIDGE_ENV[0].into(), "{bridge_port}".into()),
                    (BRIDGE_ENV[1].into(), "{bridge_token}".into()),
                    (BRIDGE_ENV[2].into(), "{bridge_origin}".into()),
                ]
                .into_iter()
                .chain(progress_env())
                .collect(),
                env_remove,
            }),
            backend_ready: ReadySignal::StdoutPrefix(BRIDGE_READY_PREFIX),
            backend_watch: &["src-tauri", "ak/ak-bridge"],
            bridge_url_param: BRIDGE_URL_PARAM,
        }
    }

    fn add_missing_ids(&self, rel_path: &str, source: &str) -> Option<String> {
        let path = rel_path.replace('\\', "/");
        if !(path.starts_with("src/") && path.ends_with(".rs")) {
            return None;
        }
        let stem = path
            .rsplit('/')
            .next()
            .and_then(|f| f.strip_suffix(".rs"))
            .unwrap_or("screen");
        let (out, added) = crate::ids::add_missing_ids(source, stem).ok()?;
        (!added.is_empty()).then_some(out)
    }

    /// Tauri's `app_data_dir()`: the identifier in tauri.conf.json.
    fn app_data_folder(&self, root: &std::path::Path) -> Option<String> {
        let conf = std::fs::read_to_string(root.join("src-tauri/tauri.conf.json")).ok()?;
        let conf: serde_json::Value = serde_json::from_str(&conf).ok()?;
        let id = conf["identifier"].as_str()?.trim();
        // A folder name, never a path.
        (!id.is_empty() && !id.contains(['/', '\\', ':']) && id != "." && id != "..")
            .then(|| id.to_owned())
    }

    fn bundle(&self) -> BundlePlan {
        // tauri.conf.json runs `trunk build --release` first (no `dev`
        // feature: the bridge is not in the release build).
        let build = |extra: &[&str]| {
            let (env, env_remove) = trunk_env();
            let mut args = strings(&["build"]);
            args.extend(strings(extra));
            CommandSpec {
                tool: Tool::CargoTauri,
                args,
                cwd: "",
                env,
                env_remove,
            }
        };
        BundlePlan {
            targets: vec![
                BundleTarget {
                    os: HostOs::Windows,
                    command: build(&["--no-bundle"]),
                    output: BundleOutput::Archive(ArchiveFormat::Zip),
                },
                BundleTarget {
                    os: HostOs::Linux,
                    command: build(&["--no-bundle"]),
                    output: BundleOutput::Archive(ArchiveFormat::TarGz),
                },
                BundleTarget {
                    os: HostOs::MacOs,
                    command: build(&["--bundles", "dmg"]),
                    output: BundleOutput::Installer {
                        kind: "dmg",
                        dir: "target/release/bundle/dmg",
                        extension: "dmg",
                    },
                },
            ],
            program: "target/release/app",
            name: JsonField {
                file: "src-tauri/tauri.conf.json",
                pointer: "/productName",
            },
            version: JsonField {
                file: "src-tauri/tauri.conf.json",
                pointer: "/version",
            },
            preview_markers: PREVIEW_MARKERS,
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
    fn the_app_data_folder_is_the_identifier() {
        let dir = tempfile::tempdir().unwrap();
        let p = TauriLeptos;
        assert_eq!(p.app_data_folder(dir.path()), None);
        std::fs::create_dir_all(dir.path().join("src-tauri")).unwrap();
        let conf = dir.path().join("src-tauri/tauri.conf.json");
        std::fs::write(&conf, r#"{ "identifier": "com.agentknife.ledger" }"#).unwrap();
        assert_eq!(
            p.app_data_folder(dir.path()).as_deref(),
            Some("com.agentknife.ledger")
        );
        std::fs::write(&conf, r#"{ "identifier": "../elsewhere" }"#).unwrap();
        assert_eq!(p.app_data_folder(dir.path()), None);
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
    fn three_js_ships_with_its_topic() {
        let pack = TauriLeptos.knowledge_pack();
        let three = pack.library("three").unwrap();
        assert_eq!(three.target, "vendor/three");
        let paths: Vec<&str> = three.files.iter().map(|f| f.path).collect();
        assert_eq!(
            paths,
            [
                "LICENSE",
                "addons/controls/OrbitControls.js",
                "three.core.js",
                "three.module.js"
            ]
        );
        // The topic's import map points at the files add_library writes.
        let topic = pack.topic("three-d").unwrap().body;
        for p in ["three.module.js", "addons/"] {
            assert!(topic.contains(&format!("./{}/{p}", three.target)), "{p}");
        }
        assert!(three.summary.contains("`three-d`"));
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
        // The build window's percentage; packaging output stays plain lines.
        let progress = ("CARGO_TERM_PROGRESS_WHEN".to_owned(), "always".to_owned());
        assert!(preview.frontend.env.contains(&progress));
        assert!(backend.env.contains(&progress));
        assert!(
            preview
                .frontend
                .args
                .windows(2)
                .any(|w| w == ["--dist", PREVIEW_DIST])
        );
        let bundle = TauriLeptos.bundle();
        for t in &bundle.targets {
            assert!(!t.command.args.iter().any(|a| a.contains("dev")));
            assert!(!t.command.env.contains(&progress));
        }
        // The installer build must not enable `dev` behind our back either.
        let conf = String::from_utf8_lossy(file("src-tauri/tauri.conf.json").unwrap().contents);
        assert!(conf.contains(r#""beforeBuildCommand": "trunk build --release""#));
    }

    #[test]
    fn debug_builds_skip_most_debug_info_and_release_is_untouched() {
        let manifest: toml::Table = toml::from_str(&String::from_utf8_lossy(
            file("Cargo.toml").unwrap().contents,
        ))
        .unwrap();
        let profile = manifest["profile"].as_table().unwrap();
        let dev = profile["dev"].as_table().unwrap();
        assert_eq!(dev["debug"].as_str(), Some("line-tables-only"));
        assert_eq!(dev["package"]["*"]["debug"].as_bool(), Some(false));
        assert!(!profile.contains_key("release"));
    }
}
