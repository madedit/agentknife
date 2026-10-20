//! One project folder and its `.agentknife/` directory.

use std::path::{Path, PathBuf};

use ak_types::chat::{Conversation, ConversationError};
use ak_types::i18n::Locale;
use ak_types::ids::{new_id, unix_now};
use ak_types::settings::{AiConnection, AiOverride};
use serde::{Deserialize, Serialize};

use crate::error::{IoContext, ProjectError};
use crate::fsutil::{write_atomic, write_if_missing};

pub const AGENTKNIFE_DIR: &str = ".agentknife";
const META_FILE: &str = "project.json";
const CONVERSATION_FILE: &str = "chat/conversation.json";
const META_SCHEMA_VERSION: u32 = 1;

const PROJECT_MD_EN: &str = include_str!("../../../templates/agentknife/PROJECT.en.md");
const PROJECT_MD_ZH_TW: &str = include_str!("../../../templates/agentknife/PROJECT.zh-TW.md");
const STATUS_MD: &str = include_str!("../../../templates/agentknife/STATUS.md");
const TASKS_MD: &str = include_str!("../../../templates/agentknife/tasks.md");
const GITIGNORE: &str = include_str!("../../../templates/agentknife/gitignore");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectMeta {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub created_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_override: Option<AiOverride>,
}

#[derive(Debug, Clone)]
pub struct Project {
    root: PathBuf,
    meta: ProjectMeta,
}

/// Result of loading the chat history.
#[derive(Debug)]
pub struct LoadedHistory {
    pub conversation: Conversation,
    /// The history file was damaged; it was moved here and a new chat started.
    pub moved_aside: Option<PathBuf>,
}

impl Project {
    /// Makes `root` a project: creates whatever part of `.agentknife/` is missing.
    /// Existing files are never overwritten (re-running is safe).
    pub fn init(root: &Path, name: &str, locale: Locale) -> Result<Project, ProjectError> {
        if !root.is_dir() {
            return Err(ProjectError::NotAFolder(root.to_path_buf()));
        }
        let dir = root.join(AGENTKNIFE_DIR);
        let project_md = match locale {
            Locale::ZhTw => PROJECT_MD_ZH_TW,
            Locale::En => PROJECT_MD_EN,
        };
        write_if_missing(
            &dir.join("PROJECT.md"),
            &project_md.replace("{{name}}", name),
        )?;
        write_if_missing(&dir.join("plan/STATUS.md"), STATUS_MD)?;
        write_if_missing(&dir.join("plan/tasks.md"), TASKS_MD)?;
        write_if_missing(&dir.join(".gitignore"), GITIGNORE)?;
        match Project::open(root) {
            Ok(p) => Ok(p),
            Err(ProjectError::NotAProject(_)) => {
                let project = Project {
                    root: root.to_path_buf(),
                    meta: ProjectMeta {
                        schema_version: META_SCHEMA_VERSION,
                        id: new_id("project"),
                        name: name.trim().to_owned(),
                        created_at: unix_now(),
                        ai_override: None,
                    },
                };
                project.save_meta()?;
                Ok(project)
            }
            Err(e) => Err(e),
        }
    }

    /// Opens an existing project.
    pub fn open(root: &Path) -> Result<Project, ProjectError> {
        let path = root.join(AGENTKNIFE_DIR).join(META_FILE);
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(ProjectError::NotAProject(root.to_path_buf()));
            }
            Err(e) => return Err(e).at(&path),
        };
        let raw: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| ProjectError::Corrupt {
                path: path.clone(),
                detail: e.to_string(),
            })?;
        if raw["schema_version"].as_u64().unwrap_or(0) > u64::from(META_SCHEMA_VERSION) {
            return Err(ProjectError::TooNew { path });
        }
        let meta = serde_json::from_value(raw).map_err(|e| ProjectError::Corrupt {
            path: path.clone(),
            detail: e.to_string(),
        })?;
        Ok(Project {
            root: root.to_path_buf(),
            meta,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn meta(&self) -> &ProjectMeta {
        &self.meta
    }

    pub fn agentknife_dir(&self) -> PathBuf {
        self.root.join(AGENTKNIFE_DIR)
    }

    pub fn rename(&mut self, name: &str) -> Result<(), ProjectError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ProjectError::EmptyName);
        }
        self.meta.name = name.to_owned();
        self.save_meta()
    }

    /// `None` follows the global AI connection again.
    pub fn set_ai_override(&mut self, ai: Option<AiOverride>) -> Result<(), ProjectError> {
        self.meta.ai_override = ai;
        self.save_meta()
    }

    /// The AI connection this project uses.
    pub fn effective_ai(&self, global: &AiConnection) -> AiConnection {
        match &self.meta.ai_override {
            Some(o) => global.with_override(o),
            None => global.clone(),
        }
    }

    fn save_meta(&self) -> Result<(), ProjectError> {
        let json = serde_json::to_string_pretty(&self.meta).map_err(|e| ProjectError::Corrupt {
            path: self.agentknife_dir().join(META_FILE),
            detail: e.to_string(),
        })?;
        write_atomic(&self.agentknife_dir().join(META_FILE), json.as_bytes())
    }

    /// Loads the chat history. A damaged file is moved aside (never deleted) and a
    /// new chat starts; a file from a newer AgentKnife is an error and left alone.
    pub fn load_conversation(&self) -> Result<LoadedHistory, ProjectError> {
        let path = self.agentknife_dir().join(CONVERSATION_FILE);
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(LoadedHistory {
                    conversation: Conversation::new(),
                    moved_aside: None,
                });
            }
            Err(e) => return Err(e).at(&path),
        };
        match Conversation::from_json(&text) {
            Ok(conversation) => Ok(LoadedHistory {
                conversation,
                moved_aside: None,
            }),
            Err(ConversationError::TooNew { .. }) => Err(ProjectError::TooNew { path }),
            Err(ConversationError::Invalid(_)) => {
                let aside =
                    path.with_file_name(format!("conversation.damaged-{}.json", unix_now()));
                std::fs::rename(&path, &aside).at(&path)?;
                Ok(LoadedHistory {
                    conversation: Conversation::new(),
                    moved_aside: Some(aside),
                })
            }
        }
    }

    pub fn save_conversation(&self, conversation: &Conversation) -> Result<(), ProjectError> {
        let path = self.agentknife_dir().join(CONVERSATION_FILE);
        let json = conversation.to_json().map_err(|e| ProjectError::Corrupt {
            path: path.clone(),
            detail: e.to_string(),
        })?;
        write_atomic(&path, json.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use ak_types::chat::Message;

    use super::*;

    #[test]
    fn init_creates_skeleton_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let p = Project::init(dir.path(), "待辦清單", Locale::ZhTw).unwrap();
        let ak = dir.path().join(".agentknife");
        let project_md = std::fs::read_to_string(ak.join("PROJECT.md")).unwrap();
        assert!(project_md.starts_with("# 待辦清單"));
        assert!(project_md.contains("這個 App 是做什麼的"));
        let status = std::fs::read_to_string(ak.join("plan/STATUS.md")).unwrap();
        assert!(
            status.contains("## Doing")
                && status.contains("## Blocked")
                && status.contains("## Done")
        );
        assert!(ak.join("plan/tasks.md").is_file());
        assert!(
            std::fs::read_to_string(ak.join(".gitignore"))
                .unwrap()
                .contains("chat/")
        );

        // The user edits PROJECT.md; re-init keeps it and keeps the id.
        std::fs::write(ak.join("PROJECT.md"), "mine").unwrap();
        let again = Project::init(dir.path(), "other", Locale::En).unwrap();
        assert_eq!(again.meta().id, p.meta().id);
        assert_eq!(
            std::fs::read_to_string(ak.join("PROJECT.md")).unwrap(),
            "mine"
        );
    }

    #[test]
    fn open_rename_and_ai_override() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            Project::open(dir.path()),
            Err(ProjectError::NotAProject(_))
        ));
        let mut p = Project::init(dir.path(), "Todo", Locale::En).unwrap();
        p.rename("Todo v2").unwrap();
        assert!(matches!(p.rename("  "), Err(ProjectError::EmptyName)));
        let global = AiConnection {
            provider: "ollama".into(),
            base_url: None,
            model: "qwen3:8b".into(),
            effort: None,
        };
        p.set_ai_override(Some(AiOverride {
            model: Some("qwen3:32b".into()),
            ..AiOverride::default()
        }))
        .unwrap();
        let reopened = Project::open(dir.path()).unwrap();
        assert_eq!(reopened.meta().name, "Todo v2");
        assert_eq!(reopened.effective_ai(&global).model, "qwen3:32b");
    }

    #[test]
    fn conversation_round_trip_and_damage_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let p = Project::init(dir.path(), "x", Locale::En).unwrap();
        assert!(
            p.load_conversation()
                .unwrap()
                .conversation
                .messages()
                .is_empty()
        );

        let mut c = Conversation::new();
        c.push_message(Message::user_text("hello"));
        p.save_conversation(&c).unwrap();
        assert_eq!(p.load_conversation().unwrap().conversation, c);

        let file = p.agentknife_dir().join(CONVERSATION_FILE);
        std::fs::write(&file, "{ not json").unwrap();
        let loaded = p.load_conversation().unwrap();
        let aside = loaded.moved_aside.expect("damaged file moved aside");
        assert!(loaded.conversation.messages().is_empty());
        assert_eq!(
            std::fs::read_to_string(aside).unwrap(),
            "{ not json",
            "kept, not deleted"
        );

        std::fs::write(&file, r#"{"schema_version": 999, "entries": []}"#).unwrap();
        assert!(matches!(
            p.load_conversation(),
            Err(ProjectError::TooNew { .. })
        ));
        assert!(file.exists(), "a newer file is left untouched");
    }
}
