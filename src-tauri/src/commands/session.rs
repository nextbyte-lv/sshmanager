use tauri::ipc::Channel;
use tauri::State;
use uuid::Uuid;

use crate::secrets;
use crate::ssh::pty::{SessionCommand, TerminalEvent};
use crate::ssh::{self};
use crate::state::{AppState, SessionHandle};
use crate::storage::{ConnectionInput, ConnectionProfile};

use super::secret_kind_for;

fn parse_id(id: &str) -> Result<Uuid, String> {
    Uuid::parse_str(id).map_err(|_| "invalid connection id".to_string())
}

#[tauri::command]
pub async fn open_session(
    state: State<'_, AppState>,
    id: String,
    cols: u16,
    rows: u16,
    on_event: Channel<TerminalEvent>,
) -> Result<String, String> {
    let uuid = parse_id(&id)?;
    let profile = {
        let store = state.connections.lock().unwrap();
        store.get(&uuid).ok_or_else(|| "connection not found".to_string())?
    };

    let secret = secrets::get_secret(&id, &profile.username, secret_kind_for(profile.auth_type))?;

    let (cmd_tx, ssh) = ssh::pty::open(profile, secret, cols, rows, on_event)
        .await
        .map_err(|e| e.to_string())?;

    let session_id = Uuid::new_v4().to_string();
    state.sessions.lock().unwrap().insert(session_id.clone(), SessionHandle { cmd_tx, ssh });
    state.connections.lock().unwrap().touch_last_used(&uuid);

    Ok(session_id)
}

#[tauri::command]
pub fn send_input(state: State<'_, AppState>, session_id: String, data: String) -> Result<(), String> {
    let sessions = state.sessions.lock().unwrap();
    let session = sessions.get(&session_id).ok_or_else(|| "session not found".to_string())?;
    session
        .cmd_tx
        .send(SessionCommand::Write(data.into_bytes()))
        .map_err(|_| "session already closed".to_string())
}

#[tauri::command]
pub fn resize_session(state: State<'_, AppState>, session_id: String, cols: u16, rows: u16) -> Result<(), String> {
    let sessions = state.sessions.lock().unwrap();
    let session = sessions.get(&session_id).ok_or_else(|| "session not found".to_string())?;
    session
        .cmd_tx
        .send(SessionCommand::Resize { cols, rows })
        .map_err(|_| "session already closed".to_string())
}

#[tauri::command]
pub fn close_session(state: State<'_, AppState>, session_id: String) -> Result<(), String> {
    if let Some(session) = state.sessions.lock().unwrap().remove(&session_id) {
        let _ = session.cmd_tx.send(SessionCommand::Close);
    }
    state.sftp.lock().unwrap().remove(&session_id);
    Ok(())
}

// Tests connection details as typed in the editor, without persisting anything to the
// connections store or Credential Manager. `id` is the existing connection's id (edit mode
// only) — used solely to fall back to its already-saved secret when `secret` is left blank
// (the "leave blank to keep" convention), never to look up or mutate stored connection fields.
#[tauri::command]
pub async fn test_connection(id: Option<String>, input: ConnectionInput, secret: Option<String>) -> Result<(), String> {
    let profile = ConnectionProfile {
        id: Uuid::nil(),
        name: input.name,
        host: input.host,
        port: input.port,
        username: input.username,
        auth_type: input.auth_type,
        key_path: input.key_path,
        tags: input.tags,
        last_used_at: None,
    };

    let secret = match secret {
        Some(s) if !s.is_empty() => Some(s),
        _ => match &id {
            Some(id) => secrets::get_secret(id, &profile.username, secret_kind_for(profile.auth_type))?,
            None => None,
        },
    };

    let session = ssh::client::connect_and_auth(&profile, secret).await.map_err(|e| e.to_string())?;
    let _ = session.disconnect(russh::Disconnect::ByApplication, "", "en").await;
    Ok(())
}
