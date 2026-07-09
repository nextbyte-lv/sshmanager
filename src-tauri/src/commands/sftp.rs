use std::sync::Arc;

use russh_sftp::client::SftpSession;
use tauri::State;

use crate::ssh::sftp::{self, SftpEntry};
use crate::state::AppState;

async fn get_sftp(state: &AppState, session_id: &str) -> Result<Arc<SftpSession>, String> {
    if let Some(existing) = state.sftp.lock().unwrap().get(session_id) {
        return Ok(existing.clone());
    }

    let ssh = {
        let sessions = state.sessions.lock().unwrap();
        let session = sessions.get(session_id).ok_or_else(|| "session not found".to_string())?;
        session.ssh.clone()
    };

    let opened = Arc::new(sftp::open_sftp(&ssh).await.map_err(|e| e.to_string())?);
    state.sftp.lock().unwrap().insert(session_id.to_string(), opened.clone());
    Ok(opened)
}

#[tauri::command]
pub async fn sftp_canonicalize(state: State<'_, AppState>, session_id: String, path: String) -> Result<String, String> {
    let session = get_sftp(&state, &session_id).await?;
    sftp::canonicalize(&session, &path).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sftp_list_dir(
    state: State<'_, AppState>,
    session_id: String,
    path: String,
) -> Result<Vec<SftpEntry>, String> {
    let session = get_sftp(&state, &session_id).await?;
    sftp::list_dir(&session, &path).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sftp_download(
    state: State<'_, AppState>,
    session_id: String,
    remote_path: String,
    local_path: String,
) -> Result<(), String> {
    let session = get_sftp(&state, &session_id).await?;
    sftp::download(&session, &remote_path, &local_path).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sftp_upload(
    state: State<'_, AppState>,
    session_id: String,
    local_path: String,
    remote_path: String,
) -> Result<(), String> {
    let session = get_sftp(&state, &session_id).await?;
    sftp::upload(&session, &local_path, &remote_path).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sftp_mkdir(state: State<'_, AppState>, session_id: String, path: String) -> Result<(), String> {
    let session = get_sftp(&state, &session_id).await?;
    sftp::make_dir(&session, &path).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sftp_delete(
    state: State<'_, AppState>,
    session_id: String,
    path: String,
    is_dir: bool,
) -> Result<(), String> {
    let session = get_sftp(&state, &session_id).await?;
    sftp::remove(&session, &path, is_dir).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sftp_rename(
    state: State<'_, AppState>,
    session_id: String,
    from: String,
    to: String,
) -> Result<(), String> {
    let session = get_sftp(&state, &session_id).await?;
    sftp::rename(&session, &from, &to).await.map_err(|e| e.to_string())
}
