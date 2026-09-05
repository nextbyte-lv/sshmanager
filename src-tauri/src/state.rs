use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime};

use notify::RecommendedWatcher;
use notify_debouncer_mini::Debouncer;
use russh_sftp::client::SftpSession;
use tauri::ipc::Channel;
use tokio::sync::mpsc::UnboundedSender;

use crate::ssh::client::Client;
use crate::ssh::monitor::{RawSample, Snapshot};
use crate::ssh::pty::SessionCommand;
use crate::ssh::sftp::FileSyncEvent;
use crate::storage::ConnectionsStore;

pub struct SessionHandle {
    pub cmd_tx: UnboundedSender<SessionCommand>,
    pub ssh: Arc<russh::client::Handle<Client>>,
    // The connection this session was opened from, so operations that need the
    // profile or its stored secret (e.g. a privileged write) can find them.
    pub connection_id: String,
}

// Identifies the exact contents of a local file cheaply, so a filesystem event can
// be told apart from a real edit. Used to ignore the write our own download made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStamp {
    pub mtime: SystemTime,
    pub size: u64,
}

impl FileStamp {
    pub fn of(meta: &std::fs::Metadata) -> Option<Self> {
        Some(Self { mtime: meta.modified().ok()?, size: meta.len() })
    }
}

pub struct WatchedFile {
    pub session_id: String,
    pub remote_path: String,
    pub on_event: Channel<FileSyncEvent>,
    // Stamp of the local file as of the last download/upload. A filesystem event
    // whose stamp still matches this means nothing was actually edited.
    pub synced: Option<FileStamp>,
}

// One host monitor per open terminal session. `previous` is the raw sample the
// next poll subtracts from; `recent` lets two polls that land together share one
// answer instead of each diffing against the other's sample -- which would halve
// the measured interval and make every rate silently wrong.
#[derive(Default)]
pub struct MonitorState {
    pub previous: Option<(Instant, RawSample)>,
    pub recent: Option<(Instant, Snapshot)>,
}

pub struct AppState {
    pub connections: Mutex<ConnectionsStore>,
    pub sessions: Mutex<HashMap<String, SessionHandle>>,
    pub sftp: Mutex<HashMap<String, Arc<SftpSession>>>,
    // A tokio mutex, because it is held across the collection await; the outer
    // std mutex is only ever locked long enough to clone the Arc out.
    pub monitor: Mutex<HashMap<String, Arc<tokio::sync::Mutex<MonitorState>>>>,
    pub file_watcher: Mutex<Debouncer<RecommendedWatcher>>,
    pub watched_dirs: Mutex<HashSet<PathBuf>>,
    pub watched_files: Mutex<HashMap<PathBuf, WatchedFile>>,
}

impl AppState {
    pub fn new(connections: ConnectionsStore, file_watcher: Debouncer<RecommendedWatcher>) -> Self {
        Self {
            connections: Mutex::new(connections),
            sessions: Mutex::new(HashMap::new()),
            sftp: Mutex::new(HashMap::new()),
            monitor: Mutex::new(HashMap::new()),
            file_watcher: Mutex::new(file_watcher),
            watched_dirs: Mutex::new(HashSet::new()),
            watched_files: Mutex::new(HashMap::new()),
        }
    }
}
