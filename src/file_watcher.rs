use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::mpsc,
};

use notify::{Event as NotifyEvent, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::runtime::error::RuntimeError;

pub struct FileWatcher {
    _watcher: RecommendedWatcher,
    receiver: mpsc::Receiver<Result<NotifyEvent, notify::Error>>,
    /// Canonical paths of all files we care about (main file + every imported file).
    watched_paths: HashSet<PathBuf>,
}

impl FileWatcher {
    /// Watch all given paths; any change to any of them will trigger a rerender.
    /// Paths are canonicalized and must exist.
    pub fn new<P, I>(paths: I) -> Result<Self, RuntimeError>
    where
        P: AsRef<Path>,
        I: IntoIterator<Item = P>,
    {
        let mut watched_paths = HashSet::new();
        let mut parents_to_watch: HashSet<PathBuf> = HashSet::new();

        for path in paths {
            let path_buf = PathBuf::from(path.as_ref());

            if !path_buf.exists() {
                return Err(RuntimeError::IoError(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("File not found: {}", path_buf.display()),
                )));
            }

            let canonical = path_buf.canonicalize().map_err(|e| {
                RuntimeError::IoError(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to canonicalize {}: {}", path_buf.display(), e),
                ))
            })?;
            watched_paths.insert(canonical.clone());

            if let Some(parent) = canonical.parent() {
                let parent_buf = parent.to_path_buf();
                parents_to_watch.insert(parent_buf);
            }
        }

        let (tx, rx) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(tx).map_err(|e| {
            RuntimeError::IoError(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to create file watcher: {}", e),
            ))
        })?;

        for parent in parents_to_watch {
            watcher
                .watch(&parent, RecursiveMode::NonRecursive)
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
            watched_paths,
        })
    }

    pub fn check_for_changes(&self) -> bool {
        let mut file_changed = false;

        while let Ok(Ok(event)) = self.receiver.try_recv() {
            match event.kind {
                EventKind::Modify(_) | EventKind::Create(_) => {
                    for p in &event.paths {
                        let canonical = p.canonicalize().unwrap_or_else(|_| p.clone());
                        if self.watched_paths.contains(&canonical) {
                            file_changed = true;
                            break;
                        }
                    }
                }
                _ => {}
            }
        }

        file_changed
    }

    pub fn paths(&self) -> &HashSet<PathBuf> {
        &self.watched_paths
    }
}
