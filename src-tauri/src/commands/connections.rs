use std::fs;

use tauri::State;
use uuid::Uuid;

use crate::secrets::{self, SecretKind};
use crate::state::AppState;
use crate::storage::{ConnectionInput, ConnectionProfile, ConnectionsExportFile, ExportedConnection};

use super::secret_kind_for;

fn parse_id(id: &str) -> Result<Uuid, String> {
    Uuid::parse_str(id).map_err(|_| "invalid connection id".to_string())
}

#[tauri::command]
pub fn list_connections(state: State<'_, AppState>) -> Vec<ConnectionProfile> {
    state.connections.lock().unwrap().list()
}

#[tauri::command]
pub fn save_connection(
    state: State<'_, AppState>,
    id: Option<String>,
    input: ConnectionInput,
) -> Result<ConnectionProfile, String> {
    let uuid = id.map(|s| parse_id(&s)).transpose()?;
    let previous = uuid.and_then(|uuid| state.connections.lock().unwrap().get(&uuid));

    let saved = state.connections.lock().unwrap().save(uuid, input).map_err(|e| e.to_string())?;

    // If the username or auth type changed, the old credential entry no longer
    // matches this profile and would otherwise be orphaned in Credential Manager.
    if let Some(previous) = previous {
        if previous.username != saved.username || previous.auth_type != saved.auth_type {
            let _ = secrets::delete_secret(&saved.id.to_string(), &previous.username, secret_kind_for(previous.auth_type));
        }
    }

    Ok(saved)
}

#[tauri::command]
pub fn delete_connection(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let uuid = parse_id(&id)?;
    let profile = state.connections.lock().unwrap().get(&uuid);
    if let Some(profile) = profile {
        let _ = secrets::delete_secret(&id, &profile.username, SecretKind::Password);
        let _ = secrets::delete_secret(&id, &profile.username, SecretKind::Passphrase);
    }
    state.connections.lock().unwrap().delete(&uuid).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn duplicate_connection(state: State<'_, AppState>, id: String) -> Result<ConnectionProfile, String> {
    let uuid = parse_id(&id)?;
    let source = state
        .connections
        .lock()
        .unwrap()
        .get(&uuid)
        .ok_or_else(|| "connection not found".to_string())?;
    let clone = state.connections.lock().unwrap().duplicate(&uuid).map_err(|e| e.to_string())?;

    let kind = secret_kind_for(source.auth_type);
    if let Ok(Some(secret)) = secrets::get_secret(&id, &source.username, kind) {
        let _ = secrets::set_secret(&clone.id.to_string(), &clone.username, kind, &secret);
    }

    Ok(clone)
}

#[tauri::command]
pub fn export_connections(
    state: State<'_, AppState>,
    path: String,
    ids: Option<Vec<String>>,
    include_secrets: bool,
) -> Result<(), String> {
    let all = state.connections.lock().unwrap().list();
    let profiles: Vec<ConnectionProfile> = match ids {
        Some(ids) => {
            let wanted = ids.iter().map(|s| parse_id(s)).collect::<Result<Vec<_>, _>>()?;
            all.into_iter().filter(|p| wanted.contains(&p.id)).collect()
        }
        None => all,
    };

    let connections = profiles
        .into_iter()
        .map(|profile| {
            let secret = if include_secrets {
                secrets::get_secret(&profile.id.to_string(), &profile.username, secret_kind_for(profile.auth_type))
                    .ok()
                    .flatten()
            } else {
                None
            };
            ExportedConnection { profile, secret }
        })
        .collect();

    let file = ConnectionsExportFile { version: 1, connections };
    let json = serde_json::to_vec_pretty(&file).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn import_connections(state: State<'_, AppState>, path: String) -> Result<Vec<ConnectionProfile>, String> {
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let file: ConnectionsExportFile = serde_json::from_str(&raw).map_err(|e| e.to_string())?;

    let mut imported = Vec::with_capacity(file.connections.len());
    for entry in file.connections {
        let input = ConnectionInput {
            name: entry.profile.name,
            host: entry.profile.host,
            port: entry.profile.port,
            username: entry.profile.username,
            auth_type: entry.profile.auth_type,
            key_path: entry.profile.key_path,
            tags: entry.profile.tags,
        };
        let saved = state.connections.lock().unwrap().save(None, input).map_err(|e| e.to_string())?;
        if let Some(secret) = entry.secret {
            let _ = secrets::set_secret(&saved.id.to_string(), &saved.username, secret_kind_for(saved.auth_type), &secret);
        }
        imported.push(saved);
    }
    Ok(imported)
}

#[tauri::command]
pub fn save_credential(id: String, username: String, kind: SecretKind, secret: String) -> Result<(), String> {
    secrets::set_secret(&id, &username, kind, &secret)
}

#[tauri::command]
pub fn has_credential(id: String, username: String, kind: SecretKind) -> Result<bool, String> {
    Ok(secrets::has_secret(&id, &username, kind))
}
