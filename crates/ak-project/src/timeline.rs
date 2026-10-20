//! Checkpoints: snapshots of the project files the user can go back to.
//!
//! Stored in a bare git repository at `.agentknife/checkpoints.git` (embedded gix,
//! no system git; the user's own `.git` is never touched). The timeline itself is an
//! append-only index in `.agentknife/timeline.json`; every entry has a ref
//! `refs/checkpoints/<id>` so it stays restorable. Going back never deletes anything:
//! unsaved work is saved as an "abandoned" entry first, and the restore is itself a
//! new entry.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ak_types::agent::{TURN_LOCK_FILE, TurnLockData};
use ak_types::ids::{new_id, unix_now};
use gix::ObjectId;
use gix::object::tree::EntryKind;
use serde::{Deserialize, Serialize};

use crate::error::{IoContext, ProjectError};
use crate::fsutil::write_atomic;
use crate::project::AGENTKNIFE_DIR;

const REPO_DIR: &str = "checkpoints.git";
const INDEX_FILE: &str = "timeline.json";
const INDEX_SCHEMA_VERSION: u32 = 1;
/// Default for [`Timeline::gc`] (a setting in P1-APP-002).
pub const DEFAULT_KEEP_TURNS: usize = 50;
/// Never part of a snapshot, wherever they are.
const SKIP_DIRS: &[&str] = &[".git", "target", "node_modules", "dist"];
/// AgentKnife's own state inside `.agentknife/`: never snapshotted or restored.
const INTERNAL: &[&str] = &[REPO_DIR, INDEX_FILE, "chat", "turn.lock"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointKind {
    /// The project as it was when AgentKnife first saw it.
    Initial,
    /// After an agent turn.
    Turn,
    /// A finished task (kept forever by checkpoint GC).
    Task,
    /// Made when an installer was built.
    Bundle,
    /// "Mark this version" by the user.
    Marked,
    /// Unsaved work set aside before going back; still restorable.
    Abandoned,
    /// Went back to an earlier entry.
    Restore,
}

impl CheckpointKind {
    /// Fluent message id for the UI's timeline.
    pub fn message_id(self) -> &'static str {
        match self {
            CheckpointKind::Initial => "timeline-initial",
            CheckpointKind::Turn => "timeline-turn",
            CheckpointKind::Task => "timeline-task",
            CheckpointKind::Bundle => "timeline-bundle",
            CheckpointKind::Marked => "timeline-marked",
            CheckpointKind::Abandoned => "timeline-abandoned",
            CheckpointKind::Restore => "timeline-restore",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineEntry {
    pub id: String,
    pub kind: CheckpointKind,
    /// Plain language, usually the user's request ("make the button blue").
    pub label: String,
    pub created_at: u64,
    /// Files changed since the previous entry, project-relative.
    pub changed_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    /// For `Restore`: the entry that was restored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restored_from: Option<String>,
    commit: String,
    tree: String,
}

/// What `Timeline::interruption` found when a project is opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interruption {
    /// A turn was running when AgentKnife stopped (crash, force-quit, error…).
    pub turn: Option<TurnLockData>,
    /// Files differ from the last checkpoint (with no lock: hand edits).
    pub unsaved_changes: bool,
}

#[derive(Serialize, Deserialize)]
struct IndexFile {
    schema_version: u32,
    entries: Vec<TimelineEntry>,
}

pub struct Timeline {
    root: PathBuf,
    repo: gix::Repository,
    entries: Vec<TimelineEntry>,
}

type Files = BTreeMap<String, (ObjectId, EntryKind)>;

fn git_err(e: impl std::fmt::Display) -> ProjectError {
    ProjectError::Corrupt {
        path: PathBuf::from(REPO_DIR),
        detail: e.to_string(),
    }
}

impl Timeline {
    /// Opens the timeline, creating it (with an `Initial` entry) on first use.
    pub fn open(project_root: &Path) -> Result<Timeline, ProjectError> {
        let ak = project_root.join(AGENTKNIFE_DIR);
        let repo_dir = ak.join(REPO_DIR);
        let repo = if repo_dir.join("HEAD").is_file() {
            gix::open(&repo_dir).map_err(git_err)?
        } else {
            std::fs::create_dir_all(&repo_dir).at(&repo_dir)?;
            gix::init_bare(&repo_dir).map_err(git_err)?
        };
        let index_path = ak.join(INDEX_FILE);
        let entries = match std::fs::read_to_string(&index_path) {
            Ok(text) => {
                let raw: serde_json::Value =
                    serde_json::from_str(&text).map_err(|e| ProjectError::Corrupt {
                        path: index_path.clone(),
                        detail: e.to_string(),
                    })?;
                if raw["schema_version"].as_u64().unwrap_or(0) > u64::from(INDEX_SCHEMA_VERSION) {
                    return Err(ProjectError::TooNew { path: index_path });
                }
                serde_json::from_value::<IndexFile>(raw)
                    .map_err(|e| ProjectError::Corrupt {
                        path: index_path.clone(),
                        detail: e.to_string(),
                    })?
                    .entries
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e).at(&index_path),
        };
        let mut timeline = Timeline {
            root: project_root.to_path_buf(),
            repo,
            entries,
        };
        if timeline.entries.is_empty() {
            timeline.record(CheckpointKind::Initial, "", None, None, true)?;
        }
        Ok(timeline)
    }

    /// Oldest first.
    pub fn entries(&self) -> &[TimelineEntry] {
        &self.entries
    }

    /// Saves the current files as a new entry. Returns `None` when nothing changed
    /// since the last entry. `Task`, `Marked` and `Bundle` always record: the marker
    /// itself matters (a task usually ends right after its last turn).
    pub fn checkpoint(
        &mut self,
        kind: CheckpointKind,
        label: &str,
        task_id: Option<&str>,
    ) -> Result<Option<TimelineEntry>, ProjectError> {
        let always = matches!(
            kind,
            CheckpointKind::Task | CheckpointKind::Marked | CheckpointKind::Bundle
        );
        self.record(kind, label, task_id, None, always)
    }

    /// Whether the files differ from the last entry.
    pub fn has_unsaved_changes(&self) -> Result<bool, ProjectError> {
        let (tree, _) = self.snapshot()?;
        Ok(Some(tree.to_string()) != self.entries.last().map(|e| e.tree.clone()))
    }

    /// What to tell the user when a project is opened.
    pub fn interruption(&self) -> Result<Option<Interruption>, ProjectError> {
        let turn = std::fs::read_to_string(self.root.join(TURN_LOCK_FILE))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok());
        let unsaved_changes = self.has_unsaved_changes()?;
        Ok((turn.is_some() || unsaved_changes).then_some(Interruption {
            turn,
            unsaved_changes,
        }))
    }

    /// Makes the files look exactly like entry `id`. Unsaved work is first kept as
    /// an `Abandoned` entry. Returns the new `Restore` entry.
    pub fn restore(&mut self, id: &str, label: &str) -> Result<TimelineEntry, ProjectError> {
        let target = self
            .entries
            .iter()
            .find(|e| e.id == id)
            .cloned()
            .ok_or_else(|| ProjectError::UnknownProject(id.to_owned()))?;
        self.record(CheckpointKind::Abandoned, label, None, None, false)?;
        let tree = ObjectId::from_hex(target.tree.as_bytes()).map_err(git_err)?;
        self.checkout(tree)?;
        let entry = self
            .record(
                CheckpointKind::Restore,
                label,
                None,
                Some(target.id.clone()),
                true,
            )?
            .ok_or_else(|| git_err("restore produced no entry"))?;
        Ok(entry)
    }

    /// "Go back to the last checkpoint" after an interruption: keeps the partial work
    /// as an `Abandoned` entry, restores the latest entry, removes the turn lock.
    pub fn discard_interrupted(
        &mut self,
        label: &str,
    ) -> Result<Option<TimelineEntry>, ProjectError> {
        let last = self
            .entries
            .iter()
            .rev()
            .find(|e| e.kind != CheckpointKind::Abandoned)
            .cloned()
            .ok_or_else(|| git_err("empty timeline"))?;
        let abandoned = self.record(CheckpointKind::Abandoned, label, None, None, false)?;
        if abandoned.is_some() {
            // Same as a normal restore: the timeline ends with the state on disk.
            let tree = ObjectId::from_hex(last.tree.as_bytes()).map_err(git_err)?;
            self.checkout(tree)?;
            self.record(
                CheckpointKind::Restore,
                label,
                None,
                Some(last.id.clone()),
                true,
            )?;
        }
        let lock = self.root.join(TURN_LOCK_FILE);
        match std::fs::remove_file(&lock) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e).at(&lock),
            _ => {}
        }
        Ok(abandoned)
    }

    /// Checkpoint GC: keeps the newest `keep_turns` `Turn` entries and folds older
    /// ones into the next surviving entry (normally their `Task`). Every other kind
    /// is kept forever. Returns how many entries were removed. Callers run it in the
    /// background.
    pub fn gc(&mut self, keep_turns: usize) -> Result<usize, ProjectError> {
        // The newest entry is the state on disk: never collected, or every file
        // would look like an unsaved change.
        let newest = self.entries.len().saturating_sub(1);
        let turns: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(i, e)| e.kind == CheckpointKind::Turn && *i != newest)
            .map(|(i, _)| i)
            .collect();
        let keep_turns = if self
            .entries
            .last()
            .is_some_and(|e| e.kind == CheckpointKind::Turn)
        {
            keep_turns.saturating_sub(1)
        } else {
            keep_turns
        };
        let excess = turns.len().saturating_sub(keep_turns);
        if excess == 0 {
            return Ok(0);
        }
        let doomed: Vec<usize> = turns[..excess].to_vec();
        let mut carried: Vec<String> = Vec::new();
        let mut kept = Vec::with_capacity(self.entries.len() - excess);
        for (i, mut entry) in std::mem::take(&mut self.entries).into_iter().enumerate() {
            if doomed.contains(&i) {
                carried.append(&mut entry.changed_files);
                if let Ok(r) = self
                    .repo
                    .find_reference(format!("refs/checkpoints/{}", entry.id).as_str())
                {
                    r.delete().map_err(git_err)?;
                }
            } else {
                if !carried.is_empty() {
                    entry.changed_files.append(&mut carried);
                    entry.changed_files.sort();
                    entry.changed_files.dedup();
                }
                kept.push(entry);
            }
        }
        // Squashed turns after the last surviving entry: nothing to fold into, and
        // nothing is lost (their state is the latest entry's state).
        self.entries = kept;
        self.save_index()?;
        Ok(excess)
    }

    fn record(
        &mut self,
        kind: CheckpointKind,
        label: &str,
        task_id: Option<&str>,
        restored_from: Option<String>,
        always: bool,
    ) -> Result<Option<TimelineEntry>, ProjectError> {
        let (tree, files) = self.snapshot()?;
        let previous = self.entries.last().cloned();
        if !always
            && previous
                .as_ref()
                .is_some_and(|p| ObjectId::from_hex(p.tree.as_bytes()).is_ok_and(|t| t == tree))
        {
            return Ok(None);
        }
        let old_files = match &previous {
            Some(p) => self.files_of(ObjectId::from_hex(p.tree.as_bytes()).map_err(git_err)?)?,
            None => Files::new(),
        };
        let changed_files = diff(&old_files, &files);
        let parent = previous
            .as_ref()
            .map(|p| ObjectId::from_hex(p.commit.as_bytes()))
            .transpose()
            .map_err(git_err)?;
        let commit = self.commit(tree, parent, label)?;
        let id = new_id("cp");
        self.repo
            .reference(
                format!("refs/checkpoints/{id}"),
                commit,
                gix::refs::transaction::PreviousValue::Any,
                "checkpoint",
            )
            .map_err(git_err)?;
        let entry = TimelineEntry {
            id,
            kind,
            label: label.to_owned(),
            created_at: unix_now(),
            changed_files,
            task_id: task_id.map(str::to_owned),
            restored_from,
            commit: commit.to_string(),
            tree: tree.to_string(),
        };
        self.entries.push(entry.clone());
        self.save_index()?;
        Ok(Some(entry))
    }

    fn commit(
        &self,
        tree: ObjectId,
        parent: Option<ObjectId>,
        label: &str,
    ) -> Result<ObjectId, ProjectError> {
        let sig = gix::actor::Signature {
            name: "AgentKnife".into(),
            email: "agentknife@localhost".into(),
            time: gix::date::Time::now_utc(),
        };
        let commit = gix::objs::Commit {
            tree,
            parents: parent.into_iter().collect(),
            author: sig.clone(),
            committer: sig,
            encoding: None,
            message: if label.is_empty() {
                "checkpoint".into()
            } else {
                label.into()
            },
            extra_headers: Vec::new(),
        };
        Ok(self.repo.write_object(&commit).map_err(git_err)?.detach())
    }

    fn save_index(&self) -> Result<(), ProjectError> {
        let path = self.root.join(AGENTKNIFE_DIR).join(INDEX_FILE);
        let file = IndexFile {
            schema_version: INDEX_SCHEMA_VERSION,
            entries: self.entries.clone(),
        };
        let json = serde_json::to_string_pretty(&file).map_err(|e| ProjectError::Corrupt {
            path: path.clone(),
            detail: e.to_string(),
        })?;
        write_atomic(&path, json.as_bytes())
    }

    /// Current project files: (tree id, path -> blob).
    fn snapshot(&self) -> Result<(ObjectId, Files), ProjectError> {
        let mut files = Files::new();
        for rel in self.worktree_files() {
            let path = self.root.join(&rel);
            let bytes = std::fs::read(&path).at(&path)?;
            let id = self.repo.write_blob(&bytes).map_err(git_err)?.detach();
            files.insert(rel, (id, entry_kind(&path)));
        }
        let mut editor = self.repo.empty_tree().edit().map_err(git_err)?;
        for (rel, (id, kind)) in &files {
            editor.upsert(rel.as_str(), *kind, *id).map_err(git_err)?;
        }
        let tree = editor.write().map_err(git_err)?.detach();
        Ok((tree, files))
    }

    /// Project-relative `/` paths of files that belong in a snapshot.
    fn worktree_files(&self) -> Vec<String> {
        let ak = self.root.join(AGENTKNIFE_DIR);
        let internal: Vec<PathBuf> = INTERNAL.iter().map(|i| ak.join(i)).collect();
        let mut walk = ignore::WalkBuilder::new(&self.root);
        walk.hidden(false)
            .require_git(false)
            .filter_entry(move |e| {
                !SKIP_DIRS.contains(&e.file_name().to_string_lossy().as_ref())
                    && !internal.iter().any(|i| e.path() == i)
            });
        let mut out: Vec<String> = walk
            .build()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
            .filter_map(|e| {
                let rel = e.path().strip_prefix(&self.root).ok()?;
                Some(
                    rel.components()
                        .map(|c| c.as_os_str().to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join("/"),
                )
            })
            .collect();
        out.sort();
        out
    }

    fn files_of(&self, tree: ObjectId) -> Result<Files, ProjectError> {
        let tree = self.repo.find_tree(tree).map_err(git_err)?;
        let mut recorder = gix::traverse::tree::Recorder::default();
        tree.traverse()
            .breadthfirst(&mut recorder)
            .map_err(git_err)?;
        Ok(recorder
            .records
            .into_iter()
            .filter(|r| r.mode.is_blob())
            .map(|r| (r.filepath.to_string(), (r.oid, r.mode.kind())))
            .collect())
    }

    /// Makes the worktree match `tree`: writes changed files, removes files that are
    /// not in it (ignored and internal files are left alone).
    fn checkout(&self, tree: ObjectId) -> Result<(), ProjectError> {
        let target = self.files_of(tree)?;
        for rel in self.worktree_files() {
            if !target.contains_key(&rel) {
                let path = self.root.join(&rel);
                std::fs::remove_file(&path).at(&path)?;
            }
        }
        for (rel, (id, _)) in &target {
            let path = self.root.join(rel);
            let data = self.repo.find_blob(*id).map_err(git_err)?.data.clone();
            if std::fs::read(&path).ok().as_deref() != Some(data.as_slice()) {
                write_atomic(&path, &data)?;
            }
        }
        Ok(())
    }
}

fn entry_kind(path: &Path) -> EntryKind {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o111 != 0) {
            return EntryKind::BlobExecutable;
        }
    }
    let _ = path;
    EntryKind::Blob
}

fn diff(old: &Files, new: &Files) -> Vec<String> {
    let mut changed: Vec<String> = new
        .iter()
        .filter(|(p, v)| old.get(*p) != Some(v))
        .map(|(p, _)| p.clone())
        .chain(old.keys().filter(|p| !new.contains_key(*p)).cloned())
        .collect();
    changed.sort();
    changed
}
