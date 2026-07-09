use std::collections::HashMap;
use std::sync::Mutex;

use tokio::sync::mpsc::UnboundedSender;

use crate::ssh::pty::SessionCommand;
use crate::storage::ConnectionsStore;

pub struct AppState {
    pub connections: Mutex<ConnectionsStore>,
    pub sessions: Mutex<HashMap<String, UnboundedSender<SessionCommand>>>,
}

impl AppState {
    pub fn new(connections: ConnectionsStore) -> Self {
        Self {
            connections: Mutex::new(connections),
            sessions: Mutex::new(HashMap::new()),
        }
    }
}
