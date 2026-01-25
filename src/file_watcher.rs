use std::{
    path::{Path, PathBuf},
    sync::mpsc,
};

use notify::{Event as NotifyEvent, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::runtime::error::RuntimeError;

pub struct FileWatcher {
    _watcher: RecommendedWatcher,
    receiver: mpsc::Receiver<Result<NotifyEvent, notify::Error>>,
    watched_path: PathBuf,
}

impl FileWatcher {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, RuntimeError> {
        let path_buf = PathBuf::from(path.as_ref());

        // Verify the file exists
        if !path_buf.exists() {
            return Err(RuntimeError::IoError(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("File not found: {}", path_buf.display()),
            )));
        }

        let (tx, rx) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(tx).map_err(|e| {
            RuntimeError::IoError(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to create file watcher: {}", e),
            ))
        })?;

        // Watch the parent directory (non-recursive) to detect changes to the file
        if let Some(parent) = path_buf.parent() {
            watcher
                .watch(parent, RecursiveMode::NonRecursive)
                .map_err(|e| {
                    RuntimeError::IoError(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Failed to watch directory: {}", e),
                    ))
                })?;
        }

        Ok(Self {
            _watcher: watcher,
            receiver: rx,
            watched_path: path_buf,
        })
    }

    pub fn check_for_changes(&self) -> bool {
        // Try to receive all pending events
        let mut file_changed = false;

        while let Ok(Ok(event)) = self.receiver.try_recv() {
            match event.kind {
                EventKind::Modify(_) | EventKind::Create(_) => {
                    // Check if the changed file matches our watched file
                    if event.paths.iter().any(|p| p == &self.watched_path) {
                        file_changed = true;
                    }
                }
                _ => {}
            }
        }

        file_changed
    }

    pub fn path(&self) -> &Path {
        &self.watched_path
    }
}
