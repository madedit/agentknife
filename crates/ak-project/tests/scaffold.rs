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
    assert_eq!(written, [".gitignore", "app.json"]);
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
