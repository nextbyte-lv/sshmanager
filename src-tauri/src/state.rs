use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use notify::RecommendedWatcher;
use notify_debouncer_mini::Debouncer;
use russh_sftp::client::SftpSession;
use tauri::ipc::Channel;
use tokio::sync::mpsc::UnboundedSender;

use crate::ssh::client::Client;
use crate::ssh::pty::SessionCommand;
use crate::ssh::sftp::FileSyncEvent;
use crate::storage::ConnectionsStore;

pub struct SessionHandle {
    pub cmd_tx: UnboundedSender<SessionCommand>,
    pub ssh: Arc<russh::client::Handle<Client>>,
}

pub struct WatchedFile {
    pub session_id: String,
    pub remote_path: String,
    pub on_event: Channel<FileSyncEvent>,
}

pub struct AppState {
    pub connections: Mutex<ConnectionsStore>,
    pub sessions: Mutex<HashMap<String, SessionHandle>>,
    pub sftp: Mutex<HashMap<String, Arc<SftpSession>>>,
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
            file_watcher: Mutex::new(file_watcher),
            watched_dirs: Mutex::new(HashSet::new()),
            watched_files: Mutex::new(HashMap::new()),
        }
    }
}
