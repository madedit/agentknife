use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProjectError {
    #[error("file system error at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{0} is not a folder")]
    NotAFolder(PathBuf),
    #[error("{0} is not an AgentKnife project")]
    NotAProject(PathBuf),
    #[error("no project with id {0}")]
    UnknownProject(String),
    #[error("a project name cannot be empty")]
    EmptyName,
    #[error("{path} is damaged: {detail}")]
    Corrupt { path: PathBuf, detail: String },
    /// Written by a newer AgentKnife: never overwrite it.
    #[error("{path} was saved by a newer version of AgentKnife")]
    TooNew { path: PathBuf },
    /// The project was created with another app stack.
    #[error("this project uses the {have} stack, not {wanted}")]
    OtherStack { have: String, wanted: String },
}

pub(crate) trait IoContext<T> {
    fn at(self, path: &std::path::Path) -> Result<T, ProjectError>;
}

impl<T> IoContext<T> for std::io::Result<T> {
    fn at(self, path: &std::path::Path) -> Result<T, ProjectError> {
        self.map_err(|source| ProjectError::Io {
            path: path.to_path_buf(),
            source,
        })
    }
}
