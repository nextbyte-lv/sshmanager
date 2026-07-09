use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use russh_sftp::client::SftpSession;
use tokio::sync::mpsc::UnboundedSender;

use crate::ssh::client::Client;
use crate::ssh::pty::SessionCommand;
use crate::storage::ConnectionsStore;

pub struct SessionHandle {
    pub cmd_tx: UnboundedSender<SessionCommand>,
    pub ssh: Arc<russh::client::Handle<Client>>,
}

pub struct AppState {
    pub connections: Mutex<ConnectionsStore>,
    pub sessions: Mutex<HashMap<String, SessionHandle>>,
    pub sftp: Mutex<HashMap<String, Arc<SftpSession>>>,
}

impl AppState {
    pub fn new(connections: ConnectionsStore) -> Self {
        Self {
            connections: Mutex::new(connections),
            sessions: Mutex::new(HashMap::new()),
            sftp: Mutex::new(HashMap::new()),
        }
    }
}
