use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};

pub struct FileWatcher {
    _watcher: RecommendedWatcher,
    rx: Receiver<notify::Result<Event>>,
    path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WatchPoll {
    pub changed: bool,
    pub errors: Vec<String>,
}

impl FileWatcher {
    pub fn new(path: &Path) -> notify::Result<Self> {
        let path = absolute_path(path);
        let parent = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let (tx, rx) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(tx)?;

        watcher.watch(&parent, RecursiveMode::NonRecursive)?;
        if path.exists() {
            let _ = watcher.watch(&path, RecursiveMode::NonRecursive);
        }

        Ok(Self {
            _watcher: watcher,
            rx,
            path,
        })
    }

    pub fn poll(&self) -> WatchPoll {
        let mut poll = WatchPoll::default();
        loop {
            match self.rx.try_recv() {
                Ok(Ok(event)) => {
                    if is_relevant_event(&event, &self.path) {
                        poll.changed = true;
                    }
                }
                Ok(Err(err)) => poll.errors.push(err.to_string()),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    poll.errors.push("file watcher disconnected".to_string());
                    break;
                }
            }
        }
        poll
    }
}

pub fn is_relevant_event(event: &Event, target: &Path) -> bool {
    let target = absolute_path(target);
    let target_parent = target.parent().map(Path::to_path_buf);
    let target_name = target.file_name().map(|name| name.to_os_string());

    event.paths.iter().any(|path| {
        let path = absolute_path(path);
        if same_path(&path, &target) {
            return true;
        }
        if let (Some(parent), Some(name)) = (&target_parent, &target_name) {
            if path.parent() == Some(parent.as_path()) && path.file_name() == Some(name.as_os_str())
            {
                return true;
            }
        }
        target_parent
            .as_deref()
            .is_some_and(|parent| same_path(&path, parent))
    })
}

pub fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

fn same_path(a: &Path, b: &Path) -> bool {
    a == b
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{DataChange, ModifyKind};
    use notify::{EventKind, RecursiveMode};
    use std::fs;
    use std::sync::mpsc;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    #[test]
    fn identifies_direct_and_parent_events_as_relevant() {
        let target = Path::new("/tmp/mdview/doc.md");
        let direct = Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Content)))
            .add_path(PathBuf::from("/tmp/mdview/doc.md"));
        let parent =
            Event::new(EventKind::Modify(ModifyKind::Any)).add_path(PathBuf::from("/tmp/mdview"));
        let other = Event::new(EventKind::Modify(ModifyKind::Any))
            .add_path(PathBuf::from("/tmp/mdview/other.md"));

        assert!(is_relevant_event(&direct, target));
        assert!(is_relevant_event(&parent, target));
        assert!(!is_relevant_event(&other, target));
    }

    #[test]
    fn notify_observes_atomic_rename_into_target_path() {
        let dir = std::env::temp_dir().join(format!(
            "mdview-watch-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("doc.md");
        let temp = dir.join(".doc.md.tmp");
        fs::write(&target, "old").unwrap();

        let (tx, rx) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(tx).unwrap();
        watcher.watch(&dir, RecursiveMode::NonRecursive).unwrap();

        fs::write(&temp, "new").unwrap();
        fs::rename(&temp, &target).unwrap();

        let deadline = Instant::now() + Duration::from_secs(3);
        let mut saw_relevant = false;
        while Instant::now() < deadline {
            if let Ok(Ok(event)) = rx.recv_timeout(Duration::from_millis(100)) {
                if is_relevant_event(&event, &target) {
                    saw_relevant = true;
                    break;
                }
            }
        }

        let _ = fs::remove_file(&target);
        let _ = fs::remove_dir(&dir);
        assert!(saw_relevant, "expected notify event for atomic rename");
    }
}
