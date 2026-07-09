use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("connection not found")]
    NotFound,
    #[error("failed to read connections file: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse connections file: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthType {
    Password,
    Key,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionProfile {
    pub id: Uuid,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_type: AuthType,
    #[serde(default)]
    pub key_path: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub last_used_at: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConnectionInput {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_type: AuthType,
    #[serde(default)]
    pub key_path: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

pub struct ConnectionsStore {
    path: PathBuf,
    profiles: Vec<ConnectionProfile>,
}

impl ConnectionsStore {
    pub fn load(path: PathBuf) -> Self {
        let profiles = match fs::read_to_string(&path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|err| {
                eprintln!("connections.json is corrupt, starting with an empty list: {err}");
                Vec::new()
            }),
            Err(_) => Vec::new(),
        };
        Self { path, profiles }
    }

    fn persist(&self) -> Result<(), StorageError> {
        let json = serde_json::to_vec_pretty(&self.profiles)?;
        fs::write(&self.path, json)?;
        Ok(())
    }

    pub fn list(&self) -> Vec<ConnectionProfile> {
        self.profiles.clone()
    }

    pub fn get(&self, id: &Uuid) -> Option<ConnectionProfile> {
        self.profiles.iter().find(|p| &p.id == id).cloned()
    }

    pub fn save(&mut self, id: Option<Uuid>, input: ConnectionInput) -> Result<ConnectionProfile, StorageError> {
        let profile = match id {
            Some(id) => {
                let existing = self.profiles.iter_mut().find(|p| p.id == id).ok_or(StorageError::NotFound)?;
                existing.name = input.name;
                existing.host = input.host;
                existing.port = input.port;
                existing.username = input.username;
                existing.auth_type = input.auth_type;
                existing.key_path = input.key_path;
                existing.tags = input.tags;
                existing.clone()
            }
            None => {
                let profile = ConnectionProfile {
                    id: Uuid::new_v4(),
                    name: input.name,
                    host: input.host,
                    port: input.port,
                    username: input.username,
                    auth_type: input.auth_type,
                    key_path: input.key_path,
                    tags: input.tags,
                    last_used_at: None,
                };
                self.profiles.push(profile.clone());
                profile
            }
        };
        self.persist()?;
        Ok(profile)
    }

    pub fn delete(&mut self, id: &Uuid) -> Result<(), StorageError> {
        let len_before = self.profiles.len();
        self.profiles.retain(|p| &p.id != id);
        if self.profiles.len() == len_before {
            return Err(StorageError::NotFound);
        }
        self.persist()
    }

    pub fn duplicate(&mut self, id: &Uuid) -> Result<ConnectionProfile, StorageError> {
        let source = self.get(id).ok_or(StorageError::NotFound)?;
        let clone = ConnectionProfile {
            id: Uuid::new_v4(),
            name: format!("{} (copy)", source.name),
            last_used_at: None,
            ..source
        };
        self.profiles.push(clone.clone());
        self.persist()?;
        Ok(clone)
    }

    pub fn touch_last_used(&mut self, id: &Uuid) {
        if let Some(profile) = self.profiles.iter_mut().find(|p| &p.id == id) {
            let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
            profile.last_used_at = Some(now_ms);
            let _ = self.persist();
        }
    }
}
