use keyring::Entry;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretKind {
    Password,
    Passphrase,
}

impl SecretKind {
    fn suffix(self) -> &'static str {
        match self {
            SecretKind::Password => "password",
            SecretKind::Passphrase => "passphrase",
        }
    }
}

fn entry(connection_id: &str, username: &str, kind: SecretKind) -> Result<Entry, String> {
    let service = format!("sshmanager:{connection_id}:{}", kind.suffix());
    Entry::new(&service, username).map_err(|e| e.to_string())
}

pub fn set_secret(connection_id: &str, username: &str, kind: SecretKind, secret: &str) -> Result<(), String> {
    entry(connection_id, username, kind)?.set_password(secret).map_err(|e| e.to_string())
}

pub fn get_secret(connection_id: &str, username: &str, kind: SecretKind) -> Result<Option<String>, String> {
    match entry(connection_id, username, kind)?.get_password() {
        Ok(secret) => Ok(Some(secret)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

pub fn delete_secret(connection_id: &str, username: &str, kind: SecretKind) -> Result<(), String> {
    match entry(connection_id, username, kind)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

pub fn has_secret(connection_id: &str, username: &str, kind: SecretKind) -> bool {
    matches!(get_secret(connection_id, username, kind), Ok(Some(_)))
}
