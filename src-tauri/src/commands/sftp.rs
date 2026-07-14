use std::path::{Path, PathBuf};
use std::sync::Arc;

use russh_sftp::client::SftpSession;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

use crate::ssh::sftp::{self, FileSyncEvent, SftpEntry, UploadEvent};
use crate::state::{AppState, WatchedFile};

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
    on_event: Channel<UploadEvent>,
) -> Result<(), String> {
    let session = get_sftp(&state, &session_id).await?;
    sftp::upload_path(&session, Path::new(&local_path), &remote_path, &|event| {
        let _ = on_event.send(event);
    })
    .await
    .map_err(|e| e.to_string())
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

// Maps a remote path onto a local cache path, mirroring the remote directory
// structure so the file keeps its name/extension for OS file-association lookup.
fn local_edit_path(app: &AppHandle, session_id: &str, remote_path: &str) -> Result<PathBuf, String> {
    if remote_path.split('/').any(|segment| segment == "..") {
        return Err("invalid remote path".to_string());
    }
    let cache_dir = app.path().app_cache_dir().map_err(|e| e.to_string())?;
    let mut local_path = cache_dir.join("sftp-edit").join(session_id);
    for segment in remote_path.split('/').filter(|s| !s.is_empty()) {
        local_path.push(segment);
    }
    Ok(local_path)
}

#[tauri::command]
pub async fn sftp_open_file(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    remote_path: String,
    on_event: Channel<FileSyncEvent>,
) -> Result<String, String> {
    let session = get_sftp(&state, &session_id).await?;
    let local_path = local_edit_path(&app, &session_id, &remote_path)?;
    if let Some(parent) = local_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| e.to_string())?;
    }
    sftp::download(&session, &remote_path, &local_path.to_string_lossy()).await.map_err(|e| e.to_string())?;

    state.watched_files.lock().unwrap().insert(
        local_path.clone(),
        WatchedFile { session_id: session_id.clone(), remote_path: remote_path.clone(), on_event },
    );

    if let Some(parent) = local_path.parent() {
        let parent = parent.to_path_buf();
        let mut dirs = state.watched_dirs.lock().unwrap();
        if dirs.insert(parent.clone()) {
            state
                .file_watcher
                .lock()
                .unwrap()
                .watcher()
                .watch(&parent, notify::RecursiveMode::NonRecursive)
                .map_err(|e| e.to_string())?;
        }
    }

    Ok(local_path.to_string_lossy().into_owned())
}

// Called (via a spawned task per changed path) from the debounced filesystem
// watcher set up in lib.rs::setup. Re-uploads a locally-edited file back to
// the remote path it was opened from, reporting progress on the channel that
// was registered when the file was opened.
pub fn handle_fs_events(app: AppHandle, events: Vec<notify_debouncer_mini::DebouncedEvent>) {
    for event in events {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            upload_watched_file(&app, &event.path).await;
        });
    }
}

async fn upload_watched_file(app: &AppHandle, path: &Path) {
    let state = app.state::<AppState>();
    let watched = {
        let files = state.watched_files.lock().unwrap();
        files.get(path).map(|w| (w.session_id.clone(), w.remote_path.clone(), w.on_event.clone()))
    };
    let Some((session_id, remote_path, on_event)) = watched else {
        return;
    };

    let _ = on_event.send(FileSyncEvent::Uploading);

    let result: Result<(), String> = async {
        let session = get_sftp(state.inner(), &session_id).await?;
        let meta = tokio::fs::metadata(path).await.map_err(|e| e.to_string())?;
        sftp::upload_file(&session, path, &remote_path, &meta, &|_| {})
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    .await;

    match result {
        Ok(()) => {
            let _ = on_event.send(FileSyncEvent::Uploaded);
        }
        Err(message) => {
            let _ = on_event.send(FileSyncEvent::Error { message });
        }
    }
}
