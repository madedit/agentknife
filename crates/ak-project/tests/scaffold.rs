//! Putting a stack's starter app into a project, with a fake stack.

// Test code: clippy's allow-unwrap-in-tests does not cover helper fns here.
#![allow(clippy::unwrap_used)]

use ak_project::{Project, ProjectError};
use ak_types::i18n::Locale;
use ak_types::stack::{
    BundlePlan, CommandSpec, CrashMarkers, JsonField, KnowledgePack, LogMarkers, PreviewPlan,
    ReadySignal, StackProfile, Template, TemplateFile, TemplateValue, Tool,
};

struct Fake(&'static str);

static FILES: &[TemplateFile] = &[
    TemplateFile {
        path: "app.json",
        contents: br#"{"name": "My App", "id": "com.agentknife.myapp"}"#,
    },
    TemplateFile {
        path: "src/main.rs",
        contents: b"fn main() {}\n",
    },
    TemplateFile {
        path: "gitignore",
        contents: b"/target/\n",
    },
    // The kit AgentKnife owns and keeps up to date.
    TemplateFile {
        path: "ak/ui/Cargo.toml",
        contents: b"[package]\nname = \"ui\"\n",
    },
    TemplateFile {
        path: "ak/ui/src/lib.rs",
        contents: b"pub fn button() {}\n",
    },
];

fn cmd() -> CommandSpec {
    CommandSpec {
        tool: Tool::Cargo,
        args: vec![],
        cwd: "",
        env: vec![],
        env_remove: vec![],
    }
}

impl StackProfile for Fake {
    fn id(&self) -> &'static str {
        self.0
    }
    fn template(&self) -> Template {
        Template {
            files: FILES,
            replace: &[
                (TemplateValue::ProductName, "My App"),
                (TemplateValue::Identifier, "com.agentknife.myapp"),
            ],
            renames: &[("gitignore", ".gitignore")],
            kit: Some("ak"),
        }
    }
    fn knowledge_pack(&self) -> KnowledgePack {
        KnowledgePack {
            id: "fake",
            version: "0",
            topics: &[],
            libraries: &[],
        }
    }
    fn doctor_requirements(&self) -> &'static [&'static str] {
        &[]
    }
    fn preview(&self) -> PreviewPlan {
        PreviewPlan {
            prepare: None,
            frontend: cmd(),
            frontend_log: LogMarkers {
                building: &[],
                success: &[],
                error: &[],
            },
            backend: None,
            backend_ready: ReadySignal::StdoutPrefix("READY"),
            backend_watch: &[],
            bridge_url_param: "x",
            backend_crash: CrashMarkers {
                start: "CRASH",
                end: "END",
            },
            data_dir: "target/data",
        }
    }
    fn bundle(&self) -> BundlePlan {
        BundlePlan {
            targets: Vec::new(),
            program: "app",
            name: JsonField {
                file: "app.json",
                pointer: "/name",
            },
            version: JsonField {
                file: "app.json",
                pointer: "/version",
            },
            preview_markers: &[],
        }
    }
}

#[test]
fn scaffold_fills_names_keeps_user_files_and_records_the_stack() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = Project::init(dir.path(), "Shopping List", Locale::En).unwrap();
    // The user already has their own main.rs: it must survive.
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/main.rs"), "// mine\n").unwrap();

    let written = project.scaffold(&Fake("fake-stack")).unwrap();
    let mut written: Vec<String> = written
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();
    written.sort();
    assert_eq!(
        written,
        [
            ".gitignore",
            "ak/ui/Cargo.toml",
            "ak/ui/src/lib.rs",
            "app.json"
        ]
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("app.json")).unwrap(),
        r#"{"name": "Shopping List", "id": "com.agentknife.shoppinglist"}"#
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("src/main.rs")).unwrap(),
        "// mine\n"
    );
    assert!(!dir.path().join("gitignore").exists());

    // Recorded, survives reopening; running again changes nothing.
    let mut reopened = Project::open(dir.path()).unwrap();
    assert_eq!(reopened.meta().stack.as_deref(), Some("fake-stack"));
    assert!(reopened.scaffold(&Fake("fake-stack")).unwrap().is_empty());

    // Another stack is refused.
    assert!(matches!(
        reopened.scaffold(&Fake("other-stack")),
        Err(ProjectError::OtherStack { .. })
    ));
}

#[test]
fn the_kit_folder_is_replaced_whole() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut project = Project::init(root, "Shop", Locale::En).unwrap();
    let fake = Fake("fake-stack");
    project.scaffold(&fake).unwrap();
    assert!(project.kit_changes(&fake).unwrap().is_empty());
    assert!(project.update_kit(&fake).unwrap().is_empty());

    // An older kit, an edit, a file of its own; the app's files are not the kit's.
    std::fs::write(
        root.join("ak/ui/src/lib.rs"),
        "pub fn button() { /* edited */ }\n",
    )
    .unwrap();
    std::fs::remove_file(root.join("ak/ui/Cargo.toml")).unwrap();
    std::fs::create_dir_all(root.join("ak/ui/src/extra")).unwrap();
    std::fs::write(
        root.join("ak/ui/src/extra/old.rs"),
        "// gone in this version\n",
    )
    .unwrap();
    std::fs::write(root.join("src/main.rs"), "// the app, changed\n").unwrap();
    let changes = [
        "ak/ui/Cargo.toml",
        "ak/ui/src/extra/old.rs",
        "ak/ui/src/lib.rs",
    ];
    assert_eq!(project.kit_changes(&fake).unwrap(), changes);
    assert_eq!(project.update_kit(&fake).unwrap(), changes);

    assert_eq!(
        std::fs::read_to_string(root.join("ak/ui/src/lib.rs")).unwrap(),
        "pub fn button() {}\n"
    );
    assert!(root.join("ak/ui/Cargo.toml").exists());
    assert!(
        !root.join("ak/ui/src/extra").exists(),
        "emptied folders go too"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("src/main.rs")).unwrap(),
        "// the app, changed\n"
    );
    assert!(project.kit_changes(&fake).unwrap().is_empty());

    // No kit folder (an app AgentKnife did not make), or another stack: nothing.
    std::fs::remove_dir_all(root.join("ak")).unwrap();
    assert!(project.kit_changes(&fake).unwrap().is_empty());
    std::fs::create_dir_all(root.join("ak")).unwrap();
    assert!(
        project
            .kit_changes(&Fake("other-stack"))
            .unwrap()
            .is_empty()
    );
}
