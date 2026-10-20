//! The list of known projects (the home screen), stored in AgentKnife's data folder.

use std::path::{Path, PathBuf};

use ak_types::i18n::Locale;
use ak_types::ids::unix_now;
use serde::{Deserialize, Serialize};

use crate::error::{IoContext, ProjectError};
use crate::fsutil::{folder_name, unique_dir, write_atomic};
use crate::project::Project;

const REGISTRY_FILE: &str = "projects.json";
const REGISTRY_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectEntry {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub created_at: u64,
    pub last_opened_at: u64,
}

/// A home-screen row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentProject {
    pub entry: ProjectEntry,
    /// False when the folder was moved or deleted outside AgentKnife.
    pub exists: bool,
}

#[derive(Serialize, Deserialize)]
struct RegistryFile {
    schema_version: u32,
    projects: Vec<ProjectEntry>,
}

pub struct Registry {
    file: PathBuf,
    default_parent: PathBuf,
    entries: Vec<ProjectEntry>,
}

impl Registry {
    /// `state_dir` holds `projects.json`; new projects go under `default_parent`
    /// unless the user picks another place.
    pub fn load(state_dir: &Path, default_parent: &Path) -> Result<Registry, ProjectError> {
        let file = state_dir.join(REGISTRY_FILE);
        let entries = match std::fs::read_to_string(&file) {
            Ok(text) => {
                let raw: serde_json::Value =
                    serde_json::from_str(&text).map_err(|e| ProjectError::Corrupt {
                        path: file.clone(),
                        detail: e.to_string(),
                    })?;
                if raw["schema_version"].as_u64().unwrap_or(0) > u64::from(REGISTRY_SCHEMA_VERSION)
                {
                    return Err(ProjectError::TooNew { path: file });
                }
                serde_json::from_value::<RegistryFile>(raw)
                    .map_err(|e| ProjectError::Corrupt {
                        path: file.clone(),
                        detail: e.to_string(),
                    })?
                    .projects
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e).at(&file),
        };
        Ok(Registry {
            file,
            default_parent: default_parent.to_path_buf(),
            entries,
        })
    }

    /// The real locations: the OS data folder, and `~/AgentKnife/Projects`.
    pub fn load_default() -> Result<Registry, ProjectError> {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let state = dirs::data_dir()
            .unwrap_or_else(|| home.clone())
            .join("AgentKnife");
        Registry::load(&state, &home.join("AgentKnife").join("Projects"))
    }

    pub fn default_parent(&self) -> &Path {
        &self.default_parent
    }

    /// Most recently opened first.
    pub fn recent(&self) -> Vec<RecentProject> {
        // Entries are kept in open order (see `remember`); walking them backwards and
        // sorting stably breaks same-second timestamp ties by real open order.
        let mut list: Vec<RecentProject> = self
            .entries
            .iter()
            .rev()
            .map(|e| RecentProject {
                exists: e.path.join(crate::AGENTKNIFE_DIR).is_dir(),
                entry: e.clone(),
            })
            .collect();
        list.sort_by_key(|r| std::cmp::Reverse(r.entry.last_opened_at));
        list
    }

    /// Creates a new project folder named after `name` (made safe and unique)
    /// under `parent`, or the default location.
    pub fn create(
        &mut self,
        name: &str,
        parent: Option<&Path>,
        locale: Locale,
    ) -> Result<Project, ProjectError> {
        if name.trim().is_empty() {
            return Err(ProjectError::EmptyName);
        }
        let parent = parent.unwrap_or(&self.default_parent).to_path_buf();
        let dir = unique_dir(&parent, &folder_name(name));
        std::fs::create_dir_all(&dir).at(&dir)?;
        let project = Project::init(&dir, name, locale)?;
        self.remember(&project)?;
        Ok(project)
    }

    /// Opens a folder: an existing project, or a plain folder that becomes one.
    pub fn open(&mut self, path: &Path, locale: Locale) -> Result<Project, ProjectError> {
        let project = match Project::open(path) {
            Err(ProjectError::NotAProject(_)) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "project".into());
                Project::init(path, &name, locale)?
            }
            other => other?,
        };
        self.remember(&project)?;
        Ok(project)
    }

    pub fn rename(&mut self, id: &str, name: &str) -> Result<(), ProjectError> {
        let entry = self
            .entries
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or_else(|| ProjectError::UnknownProject(id.to_owned()))?;
        let mut project = Project::open(&entry.path)?;
        project.rename(name)?;
        entry.name = project.meta().name.clone();
        self.save()
    }

    /// Removes a project from the list. Its files stay where they are.
    pub fn forget(&mut self, id: &str) -> Result<(), ProjectError> {
        let before = self.entries.len();
        self.entries.retain(|e| e.id != id);
        if self.entries.len() == before {
            return Err(ProjectError::UnknownProject(id.to_owned()));
        }
        self.save()
    }

    fn remember(&mut self, project: &Project) -> Result<(), ProjectError> {
        let meta = project.meta();
        // Move to the end: the list doubles as open order.
        self.entries.retain(|e| e.id != meta.id);
        self.entries.push(ProjectEntry {
            id: meta.id.clone(),
            name: meta.name.clone(),
            path: project.root().to_path_buf(),
            created_at: meta.created_at,
            last_opened_at: unix_now(),
        });
        self.save()
    }

    fn save(&self) -> Result<(), ProjectError> {
        let file = RegistryFile {
            schema_version: REGISTRY_SCHEMA_VERSION,
            projects: self.entries.clone(),
        };
        let json = serde_json::to_string_pretty(&file).map_err(|e| ProjectError::Corrupt {
            path: self.file.clone(),
            detail: e.to_string(),
        })?;
        write_atomic(&self.file, json.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Env {
        state: tempfile::TempDir,
        projects: tempfile::TempDir,
    }

    impl Env {
        fn new() -> Self {
            Env {
                state: tempfile::tempdir().unwrap(),
                projects: tempfile::tempdir().unwrap(),
            }
        }

        fn registry(&self) -> Registry {
            Registry::load(self.state.path(), self.projects.path()).unwrap()
        }
    }

    #[test]
    fn create_list_rename_forget_survive_restart() {
        let env = Env::new();
        let mut reg = env.registry();
        let a = reg.create("待辦清單", None, Locale::ZhTw).unwrap();
        let b = reg.create("待辦清單", None, Locale::ZhTw).unwrap();
        assert_eq!(a.root(), env.projects.path().join("待辦清單"));
        assert_eq!(b.root(), env.projects.path().join("待辦清單 2"));
        assert!(matches!(
            reg.create(" ", None, Locale::En),
            Err(ProjectError::EmptyName)
        ));

        reg.rename(&a.meta().id, "購物清單").unwrap();
        // "Restart": a fresh registry from disk.
        let mut reg = env.registry();
        let names: Vec<String> = reg.recent().into_iter().map(|r| r.entry.name).collect();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"購物清單".to_owned()));
        assert_eq!(Project::open(a.root()).unwrap().meta().name, "購物清單");

        reg.forget(&b.meta().id).unwrap();
        assert_eq!(env.registry().recent().len(), 1);
        assert!(
            b.root().join(".agentknife").is_dir(),
            "forget keeps the files"
        );
    }

    #[test]
    fn custom_location_open_existing_and_adopt_folder() {
        let env = Env::new();
        let elsewhere = tempfile::tempdir().unwrap();
        let mut reg = env.registry();
        let p = reg
            .create("Game", Some(elsewhere.path()), Locale::En)
            .unwrap();
        assert_eq!(p.root(), elsewhere.path().join("Game"));

        // Opening it again updates the same entry.
        let again = reg.open(p.root(), Locale::En).unwrap();
        assert_eq!(again.meta().id, p.meta().id);
        assert_eq!(reg.recent().len(), 1);

        // A plain folder is adopted as a project.
        let plain = elsewhere.path().join("old-site");
        std::fs::create_dir_all(&plain).unwrap();
        std::fs::write(plain.join("index.html"), "<h1>hi</h1>").unwrap();
        let adopted = reg.open(&plain, Locale::En).unwrap();
        assert_eq!(adopted.meta().name, "old-site");
        assert!(plain.join("index.html").is_file());
        assert_eq!(
            reg.recent()[0].entry.id,
            adopted.meta().id,
            "most recent first"
        );
    }

    #[test]
    fn missing_folders_are_flagged_not_dropped() {
        let env = Env::new();
        let mut reg = env.registry();
        let p = reg.create("Temp", None, Locale::En).unwrap();
        std::fs::remove_dir_all(p.root()).unwrap();
        let recent = env.registry().recent();
        assert_eq!(recent.len(), 1);
        assert!(!recent[0].exists);
    }
}
