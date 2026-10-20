//! Checkpoint timeline in temp projects.

// Test code: clippy's allow-unwrap-in-tests does not cover helper fns here.
#![allow(clippy::unwrap_used)]

use std::path::Path;

use ak_project::{CheckpointKind, Project, Timeline};
use ak_types::i18n::Locale;

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn read(root: &Path, rel: &str) -> Option<String> {
    std::fs::read_to_string(root.join(rel)).ok()
}

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "src/app.rs", "button: red");
    Project::init(dir.path(), "todo", Locale::En).unwrap();
    dir
}

#[test]
fn every_kind_has_a_localized_label() {
    for locale in Locale::ALL {
        let i18n = ak_types::i18n::I18n::new(locale).unwrap();
        for kind in [
            CheckpointKind::Initial,
            CheckpointKind::Turn,
            CheckpointKind::Task,
            CheckpointKind::Bundle,
            CheckpointKind::Marked,
            CheckpointKind::Abandoned,
            CheckpointKind::Restore,
        ] {
            assert_ne!(
                i18n.t(kind.message_id()),
                kind.message_id(),
                "{locale} {kind:?}"
            );
        }
    }
}

#[test]
fn checkpoints_only_when_something_changed() {
    let dir = project();
    let root = dir.path();
    let mut t = Timeline::open(root).unwrap();
    assert_eq!(t.entries().len(), 1);
    assert_eq!(t.entries()[0].kind, CheckpointKind::Initial);
    assert!(
        t.entries()[0]
            .changed_files
            .contains(&"src/app.rs".to_owned())
    );

    assert!(
        t.checkpoint(CheckpointKind::Turn, "nothing", None)
            .unwrap()
            .is_none()
    );
    write(root, "src/app.rs", "button: blue");
    write(root, "src/新檔案.rs", "// 你好");
    let e = t
        .checkpoint(CheckpointKind::Turn, "把按鈕改成藍色", None)
        .unwrap()
        .unwrap();
    assert_eq!(e.label, "把按鈕改成藍色");
    assert_eq!(e.changed_files, ["src/app.rs", "src/新檔案.rs"]);
    assert!(!t.has_unsaved_changes().unwrap());

    // Marked always records, even without changes.
    let m = t
        .checkpoint(CheckpointKind::Marked, "v1 done", None)
        .unwrap()
        .unwrap();
    assert!(m.changed_files.is_empty());
    let task = t
        .checkpoint(CheckpointKind::Task, "", Some("T-001"))
        .unwrap();
    let task = task.expect("a task checkpoint is recorded even without new changes");
    assert!(task.changed_files.is_empty());
    assert_eq!(task.task_id.as_deref(), Some("T-001"));

    // The index survives reopening.
    let again = Timeline::open(root).unwrap();
    assert_eq!(again.entries(), t.entries());
}

#[test]
fn restore_goes_back_without_losing_anything() {
    let dir = project();
    let root = dir.path();
    let mut t = Timeline::open(root).unwrap();
    let red = t.entries()[0].id.clone();
    write(root, "src/app.rs", "button: blue");
    t.checkpoint(CheckpointKind::Turn, "blue", None)
        .unwrap()
        .unwrap();

    // Unsaved work on top: an edit and a new file.
    write(root, "src/app.rs", "button: green (unsaved)");
    write(root, "src/extra.rs", "extra");
    let restore = t.restore(&red, "Go back to the start").unwrap();
    assert_eq!(restore.kind, CheckpointKind::Restore);
    assert_eq!(restore.restored_from.as_deref(), Some(red.as_str()));
    assert_eq!(read(root, "src/app.rs").as_deref(), Some("button: red"));
    assert!(
        read(root, "src/extra.rs").is_none(),
        "files not in the checkpoint are removed"
    );

    // The unsaved work was kept as an abandoned entry and can come back.
    let abandoned = t
        .entries()
        .iter()
        .find(|e| e.kind == CheckpointKind::Abandoned)
        .unwrap()
        .clone();
    assert_eq!(abandoned.changed_files, ["src/app.rs", "src/extra.rs"]);
    t.restore(&abandoned.id, "Bring back my unsaved work")
        .unwrap();
    assert_eq!(
        read(root, "src/app.rs").as_deref(),
        Some("button: green (unsaved)")
    );
    assert_eq!(read(root, "src/extra.rs").as_deref(), Some("extra"));

    // Append-only: nothing was removed from the timeline.
    let kinds: Vec<CheckpointKind> = t.entries().iter().map(|e| e.kind).collect();
    assert_eq!(
        kinds,
        [
            CheckpointKind::Initial,
            CheckpointKind::Turn,
            CheckpointKind::Abandoned,
            CheckpointKind::Restore,
            CheckpointKind::Restore,
        ]
    );
}

#[test]
fn restore_leaves_ignored_internal_and_user_git_files_alone() {
    let dir = project();
    let root = dir.path();
    write(root, ".gitignore", "*.log\n");
    write(root, ".git/HEAD", "ref: refs/heads/main\n"); // the user's own repository
    let mut t = Timeline::open(root).unwrap();
    let start = t.entries()[0].id.clone();

    write(root, "target/debug/app.exe", "binary");
    write(root, "build.log", "log");
    write(root, ".agentknife/chat/conversation.json", "{\"chat\": 1}");
    write(root, "src/app.rs", "changed");
    t.checkpoint(CheckpointKind::Turn, "change", None).unwrap();
    write(root, ".agentknife/chat/conversation.json", "{\"chat\": 2}");

    t.restore(&start, "back").unwrap();
    assert_eq!(read(root, "src/app.rs").as_deref(), Some("button: red"));
    assert_eq!(
        read(root, "target/debug/app.exe").as_deref(),
        Some("binary")
    );
    assert_eq!(read(root, "build.log").as_deref(), Some("log"));
    assert_eq!(
        read(root, ".git/HEAD").as_deref(),
        Some("ref: refs/heads/main\n")
    );
    assert_eq!(
        read(root, ".agentknife/chat/conversation.json").as_deref(),
        Some("{\"chat\": 2}"),
        "the chat is never rewound"
    );
    assert!(
        t.entries()
            .iter()
            .all(|e| !e.changed_files.iter().any(|f| f.starts_with("target/")
                || f.starts_with(".git/")
                || f.contains("chat/")
                || f.ends_with(".log")))
    );
}

/// Our hand-built commits must be valid git: a real git (when installed) fscks the
/// repository strictly and sees every checkpoint. AgentKnife itself never needs git.
#[test]
fn checkpoint_repository_is_valid_git() {
    let git_available = std::process::Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !git_available {
        return;
    }
    let dir = project();
    let root = dir.path();
    let mut t = Timeline::open(root).unwrap();
    write(root, "src/app.rs", "blue");
    write(root, "資料/說明.txt", "中文檔名");
    t.checkpoint(CheckpointKind::Turn, "改成藍色", None)
        .unwrap();
    let first = t.entries()[0].id.clone();
    t.restore(&first, "back").unwrap();

    let repo = root.join(".agentknife/checkpoints.git");
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("--git-dir")
            .arg(&repo)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    git(&["fsck", "--strict", "--no-dangling"]);
    let refs = git(&["for-each-ref", "--format=%(refname)", "refs/checkpoints/"]);
    assert_eq!(refs.lines().count(), t.entries().len());
    let tree = git(&[
        "ls-tree",
        "-r",
        "--name-only",
        "-z",
        &format!("refs/checkpoints/{}", t.entries()[1].id),
    ]);
    let names: Vec<&str> = tree.split('\0').filter(|s| !s.is_empty()).collect();
    assert!(names.contains(&"資料/說明.txt"), "{names:?}");
    assert!(
        names
            .iter()
            .all(|n| !n.contains("checkpoints.git") && !n.contains("timeline.json"))
    );
}

#[test]
fn interrupted_turn_is_detected_and_can_be_discarded() {
    let dir = project();
    let root = dir.path();
    let mut t = Timeline::open(root).unwrap();
    assert_eq!(t.interruption().unwrap(), None);

    // Hand edits without a lock: unsaved changes, but not an interrupted turn.
    write(root, "src/app.rs", "edited in the code editor");
    let i = t.interruption().unwrap().unwrap();
    assert!(i.turn.is_none() && i.unsaved_changes);
    t.checkpoint(CheckpointKind::Turn, "manual edit", None)
        .unwrap();

    // A turn that died halfway.
    write(
        root,
        ".agentknife/turn.lock",
        r#"{"turn_id":"t1","started_at_unix":1,"request":"加上刪除按鈕","touched_files":["src/app.rs"],"step":2}"#,
    );
    write(root, "src/app.rs", "half-written delete button");
    let i = t.interruption().unwrap().unwrap();
    assert_eq!(i.turn.as_ref().unwrap().request, "加上刪除按鈕");
    assert!(i.unsaved_changes);

    let abandoned = t
        .discard_interrupted("加上刪除按鈕 (interrupted)")
        .unwrap()
        .unwrap();
    assert_eq!(abandoned.kind, CheckpointKind::Abandoned);
    assert_eq!(
        read(root, "src/app.rs").as_deref(),
        Some("edited in the code editor")
    );
    assert!(!root.join(".agentknife/turn.lock").exists());
    assert_eq!(t.interruption().unwrap(), None);
    assert_eq!(t.entries().last().unwrap().kind, CheckpointKind::Restore);

    // The half-done work is still there to bring back.
    t.restore(&abandoned.id, "bring it back").unwrap();
    assert_eq!(
        read(root, "src/app.rs").as_deref(),
        Some("half-written delete button")
    );
}
