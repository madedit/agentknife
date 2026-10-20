//! Checkpoint GC retention rules (P1-PROJ-004).

// Test code: clippy's allow-unwrap-in-tests does not cover helper fns here.
#![allow(clippy::unwrap_used)]

use std::path::Path;

use ak_project::{CheckpointKind, Project, Timeline};
use ak_types::i18n::Locale;

fn setup() -> (tempfile::TempDir, Timeline) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "0").unwrap();
    Project::init(dir.path(), "gc", Locale::En).unwrap();
    let t = Timeline::open(dir.path()).unwrap();
    (dir, t)
}

/// One turn that changes `file`.
fn turn(root: &Path, t: &mut Timeline, n: usize, file: &str) {
    std::fs::write(root.join(file), n.to_string()).unwrap();
    t.checkpoint(CheckpointKind::Turn, &format!("turn {n}"), None)
        .unwrap()
        .unwrap();
}

fn count(t: &Timeline, kind: CheckpointKind) -> usize {
    t.entries().iter().filter(|e| e.kind == kind).count()
}

#[test]
fn keeps_newest_turns_and_every_other_kind() {
    let (dir, mut t) = setup();
    let root = dir.path();
    for n in 1..=30 {
        turn(root, &mut t, n, "a.txt");
    }
    turn(root, &mut t, 31, "old-feature.txt");
    t.checkpoint(CheckpointKind::Task, "Task 1 done", Some("T-001"))
        .unwrap();
    t.checkpoint(CheckpointKind::Marked, "v1", None).unwrap();
    for n in 32..=60 {
        turn(root, &mut t, n, "b.txt");
    }
    std::fs::write(root.join("a.txt"), "unsaved").unwrap();
    let first = t.entries()[0].id.clone();
    t.restore(&first, "back").unwrap(); // adds abandoned + restore
    t.checkpoint(CheckpointKind::Bundle, "installer", None)
        .unwrap();

    let before = t.entries().len();
    let removed = t.gc(50).unwrap();
    assert_eq!(removed, 10);
    assert_eq!(t.entries().len(), before - 10);
    assert_eq!(count(&t, CheckpointKind::Turn), 50);
    for kind in [
        CheckpointKind::Initial,
        CheckpointKind::Task,
        CheckpointKind::Marked,
        CheckpointKind::Abandoned,
        CheckpointKind::Restore,
        CheckpointKind::Bundle,
    ] {
        assert_eq!(count(&t, kind), 1, "{kind:?} is never collected");
    }
    // The oldest turns went; the newest stayed.
    let labels: Vec<&str> = t.entries().iter().map(|e| e.label.as_str()).collect();
    assert!(!labels.contains(&"turn 1") && !labels.contains(&"turn 10"));
    assert!(labels.contains(&"turn 11") && labels.contains(&"turn 60"));

    // GC is idempotent and survives reopening; kept entries still restore.
    assert_eq!(t.gc(50).unwrap(), 0);
    let mut reopened = Timeline::open(root).unwrap();
    assert_eq!(reopened.entries(), t.entries());
    let turn_20 = reopened
        .entries()
        .iter()
        .find(|e| e.label == "turn 20")
        .unwrap()
        .id
        .clone();
    reopened.restore(&turn_20, "to 20").unwrap();
    assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "20");
}

#[test]
fn squashed_changes_fold_into_the_task() {
    let (dir, mut t) = setup();
    let root = dir.path();
    turn(root, &mut t, 1, "login.rs");
    turn(root, &mut t, 2, "style.css");
    std::fs::write(root.join("README.md"), "done").unwrap();
    let task = t
        .checkpoint(CheckpointKind::Task, "Login page", Some("T-001"))
        .unwrap()
        .unwrap();
    assert_eq!(task.changed_files, ["README.md"]);
    turn(root, &mut t, 3, "later.rs");

    assert_eq!(t.gc(1).unwrap(), 2);
    let task = t
        .entries()
        .iter()
        .find(|e| e.kind == CheckpointKind::Task)
        .unwrap();
    assert_eq!(task.changed_files, ["README.md", "login.rs", "style.css"]);
    assert_eq!(count(&t, CheckpointKind::Turn), 1);
}

#[test]
fn keep_zero_and_nothing_to_collect() {
    let (dir, mut t) = setup();
    assert_eq!(t.gc(50).unwrap(), 0, "only the initial entry");
    turn(dir.path(), &mut t, 1, "x.txt");
    turn(dir.path(), &mut t, 2, "x.txt");
    // Even with keep 0 the newest entry stays: it is the state on disk.
    assert_eq!(t.gc(0).unwrap(), 1);
    assert_eq!(count(&t, CheckpointKind::Turn), 1);
    assert_eq!(t.entries().last().unwrap().label, "turn 2");
    // Files on disk are untouched by GC, and nothing looks unsaved afterwards.
    assert_eq!(
        std::fs::read_to_string(dir.path().join("x.txt")).unwrap(),
        "2"
    );
    assert!(!t.has_unsaved_changes().unwrap());
}

#[test]
fn newest_turn_counts_towards_the_limit() {
    let (dir, mut t) = setup();
    for n in 1..=5 {
        turn(dir.path(), &mut t, n, "x.txt");
    }
    assert_eq!(t.gc(3).unwrap(), 2);
    assert_eq!(count(&t, CheckpointKind::Turn), 3);
    assert!(!t.has_unsaved_changes().unwrap());
}
