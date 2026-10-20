//! `.agentknife/turn.lock`: marks a turn in progress so an interrupted turn can be
//! detected and resumed (AGENTS.md §4, "Interrupted turns").

use std::io;
use std::path::{Path, PathBuf};

pub use ak_types::agent::{TURN_LOCK_FILE as LOCK_FILE, TurnLockData};

pub struct TurnLock {
    path: PathBuf,
    data: TurnLockData,
}

impl TurnLock {
    pub fn acquire(project_dir: &Path, turn_id: &str, request: &str) -> io::Result<Self> {
        let lock = Self {
            path: project_dir.join(LOCK_FILE),
            data: TurnLockData {
                turn_id: turn_id.to_owned(),
                started_at_unix: ak_types::ids::unix_now(),
                request: request.to_owned(),
                touched_files: Vec::new(),
                step: 0,
            },
        };
        lock.write()?;
        Ok(lock)
    }

    /// Takes over the lock of an interrupted turn, to finish it (`None` if none).
    pub fn resume(project_dir: &Path) -> io::Result<Option<Self>> {
        Ok(Self::read(project_dir).map(|data| Self {
            path: project_dir.join(LOCK_FILE),
            data,
        }))
    }

    /// Reads a lock left behind by an interrupted turn.
    pub fn read(project_dir: &Path) -> Option<TurnLockData> {
        let text = std::fs::read_to_string(project_dir.join(LOCK_FILE)).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn record_step(&mut self, touched: &[String]) -> io::Result<()> {
        self.data.step += 1;
        for t in touched {
            if !self.data.touched_files.contains(t) {
                self.data.touched_files.push(t.clone());
            }
        }
        self.write()
    }

    pub fn data(&self) -> &TurnLockData {
        &self.data
    }

    /// Call only after the turn's checkpoint exists.
    pub fn release(self) -> io::Result<()> {
        match std::fs::remove_file(&self.path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    fn write(&self) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(&self.data).map_err(io::Error::other)?;
        // Write-then-rename so a crash never leaves a half-written lock.
        let tmp = self.path.with_extension("lock.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, &self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let mut lock = TurnLock::acquire(dir.path(), "t1", "make it blue").unwrap();
        lock.record_step(&["src/app.rs".into()]).unwrap();
        lock.record_step(&["src/app.rs".into(), "style.css".into()])
            .unwrap();
        let read = TurnLock::read(dir.path()).unwrap();
        assert_eq!(read.request, "make it blue");
        assert_eq!(read.step, 2);
        assert_eq!(read.touched_files, ["src/app.rs", "style.css"]);
        lock.release().unwrap();
        assert!(TurnLock::read(dir.path()).is_none());
    }
}
