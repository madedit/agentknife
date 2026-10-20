//! A new project gets the default stack's starter app with its own names.
//! (Building it is checked by hand: see P1-TPL-001 in plan/STATUS.md.)

// Test code: clippy's allow-unwrap-in-tests does not cover helper fns here.
#![allow(clippy::unwrap_used)]

use ak_preview::profiles;
use ak_project::Registry;
use ak_types::i18n::Locale;

#[test]
fn new_project_gets_the_default_stack() {
    let home = tempfile::tempdir().unwrap();
    let mut registry = Registry::load(&home.path().join("state"), home.path()).unwrap();
    let mut project = registry
        .create("待辦清單 Todo", None, Locale::ZhTw)
        .unwrap();
    let profile = profiles::default_profile();
    let written = project.scaffold(profile).unwrap();
    assert!(written.len() > 30, "{written:?}");

    let root = project.root();
    let conf = std::fs::read_to_string(root.join("src-tauri/tauri.conf.json")).unwrap();
    assert!(conf.contains(r#""productName": "待辦清單 Todo""#), "{conf}");
    assert!(conf.contains(r#""identifier": "com.agentknife.todo""#));
    assert!(
        std::fs::read_to_string(root.join("index.html"))
            .unwrap()
            .contains("<title>待辦清單 Todo</title>")
    );
    for path in [
        ".gitignore",
        "Cargo.lock",
        "ak/ak-bridge/Cargo.toml",
        "src-tauri/icons/icon.ico",
    ] {
        assert!(root.join(path).is_file(), "{path}");
    }
    assert!(!root.join("template.toml").exists());
    // The project remembers its stack, so AgentKnife finds the right profile again.
    let stored = project.meta().stack.clone().unwrap();
    assert_eq!(profiles::by_id(&stored).map(|p| p.id()), Some(profile.id()));
    // Binary files arrive byte for byte.
    let ico = std::fs::read(root.join("src-tauri/icons/icon.ico")).unwrap();
    let original = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../templates/tauri-leptos/src-tauri/icons/icon.ico"
    ))
    .unwrap();
    assert_eq!(ico, original);
}
