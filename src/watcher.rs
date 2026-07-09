use notify::event::{AccessKind, AccessMode, MetadataKind, ModifyKind};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant, SystemTime};

const METADATA_POLL_INTERVAL: Duration = Duration::from_secs(10);

pub struct FileWatcher {
    _watcher: RecommendedWatcher,
    rx: Receiver<notify::Result<Event>>,
    path: PathBuf,
    signature: FileSignature,
    next_metadata_poll: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WatchPoll {
    pub changed: bool,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileSignature {
    exists: bool,
    dev: Option<u64>,
    ino: Option<u64>,
    len: Option<u64>,
    modified: Option<SystemTime>,
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
            signature: FileSignature::read(&path),
            next_metadata_poll: Instant::now() + METADATA_POLL_INTERVAL,
            path,
        })
    }

    pub fn poll(&mut self) -> WatchPoll {
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

        if poll.changed {
            self.signature = FileSignature::read(&self.path);
            self.next_metadata_poll = Instant::now() + METADATA_POLL_INTERVAL;
            return poll;
        }

        if Instant::now() >= self.next_metadata_poll {
            let signature = FileSignature::read(&self.path);
            if signature != self.signature {
                self.signature = signature;
                poll.changed = true;
            }
            self.next_metadata_poll = Instant::now() + METADATA_POLL_INTERVAL;
        }

        poll
    }
}

impl FileSignature {
    fn read(path: &Path) -> Self {
        match fs::metadata(path) {
            Ok(metadata) => Self {
                exists: true,
                dev: Some(metadata.dev()),
                ino: Some(metadata.ino()),
                len: Some(metadata.len()),
                modified: metadata.modified().ok(),
            },
            Err(_) => Self {
                exists: false,
                dev: None,
                ino: None,
                len: None,
                modified: None,
            },
        }
    }
}

pub fn is_relevant_event(event: &Event, target: &Path) -> bool {
    if !is_reload_event_kind(event.kind) {
        return false;
    }

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
        false
    })
}

pub fn is_reload_event_kind(kind: EventKind) -> bool {
    match kind {
        EventKind::Any | EventKind::Create(_) | EventKind::Remove(_) => true,
        EventKind::Modify(ModifyKind::Any | ModifyKind::Data(_) | ModifyKind::Name(_)) => true,
        EventKind::Modify(ModifyKind::Metadata(MetadataKind::WriteTime) | ModifyKind::Other) => {
            true
        }
        EventKind::Access(AccessKind::Close(AccessMode::Write)) => true,
        EventKind::Access(_) | EventKind::Modify(ModifyKind::Metadata(_)) | EventKind::Other => {
            false
        }
    }
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
    use notify::event::{AccessKind, AccessMode, DataChange, MetadataKind, ModifyKind};
    use notify::{EventKind, RecursiveMode};
    use std::fs;
    use std::sync::mpsc;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    #[test]
    fn identifies_only_mutating_target_events_as_relevant() {
        let target = Path::new("/tmp/mdview/doc.md");
        let direct = Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Content)))
            .add_path(PathBuf::from("/tmp/mdview/doc.md"));
        let close_write = Event::new(EventKind::Access(AccessKind::Close(AccessMode::Write)))
            .add_path(PathBuf::from("/tmp/mdview/doc.md"));
        let read_close = Event::new(EventKind::Access(AccessKind::Close(AccessMode::Read)))
            .add_path(PathBuf::from("/tmp/mdview/doc.md"));
        let access_time = Event::new(EventKind::Modify(ModifyKind::Metadata(
            MetadataKind::AccessTime,
        )))
        .add_path(PathBuf::from("/tmp/mdview/doc.md"));
        let write_time = Event::new(EventKind::Modify(ModifyKind::Metadata(
            MetadataKind::WriteTime,
        )))
        .add_path(PathBuf::from("/tmp/mdview/doc.md"));
        let parent =
            Event::new(EventKind::Modify(ModifyKind::Any)).add_path(PathBuf::from("/tmp/mdview"));
        let other = Event::new(EventKind::Modify(ModifyKind::Any))
            .add_path(PathBuf::from("/tmp/mdview/other.md"));

        assert!(is_relevant_event(&direct, target));
        assert!(is_relevant_event(&close_write, target));
        assert!(is_relevant_event(&write_time, target));
        assert!(!is_relevant_event(&read_close, target));
        assert!(!is_relevant_event(&access_time, target));
        assert!(!is_relevant_event(&parent, target));
        assert!(!is_relevant_event(&other, target));
    }

    #[test]
    fn reading_watched_file_does_not_trigger_reload() {
        let dir = std::env::temp_dir().join(format!(
            "mdview-watch-read-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("doc.md");
        fs::write(&target, "content").unwrap();
        let mut watcher = FileWatcher::new(&target).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        let _ = watcher.poll();

        let _ = fs::read_to_string(&target).unwrap();
        let deadline = Instant::now() + Duration::from_millis(700);
        let mut changed = false;
        while Instant::now() < deadline {
            if watcher.poll().changed {
                changed = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }

        let _ = fs::remove_file(&target);
        let _ = fs::remove_dir(&dir);
        assert!(
            !changed,
            "reading the watched file must not schedule reload"
        );
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

    #[test]
    fn metadata_poll_detects_recreate_when_notify_events_are_missing() {
        let dir = std::env::temp_dir().join(format!(
            "mdview-watch-recreate-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("doc.md");
        fs::write(&target, "old").unwrap();
        let mut watcher = FileWatcher::new(&target).unwrap();
        let old_signature = watcher.signature.clone();

        fs::remove_file(&target).unwrap();
        fs::write(&target, "new recreated content").unwrap();
        while watcher.rx.try_recv().is_ok() {}

        watcher.signature = old_signature;
        watcher.next_metadata_poll = Instant::now();
        let poll = watcher.poll();

        let _ = fs::remove_file(&target);
        let _ = fs::remove_dir(&dir);
        assert!(
            poll.changed,
            "metadata fallback should detect recreated file"
        );
    }
}
