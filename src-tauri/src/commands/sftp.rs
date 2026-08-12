use std::path::{Path, PathBuf};
use std::sync::Arc;

use russh_sftp::client::SftpSession;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

use crate::secrets::{self, SecretKind};
use crate::ssh::client::Client;
use crate::ssh::sftp::{self, FileSyncEvent, SftpEntry, UploadEvent};
use crate::ssh::{self};
use crate::state::{AppState, FileStamp, WatchedFile};
use crate::storage::AuthType;

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
    match sftp::remove(&session, &path, is_dir).await {
        Ok(()) => Ok(()),
        // Same escalation rule as the write path: retry under sudo only when the
        // server refused for lack of permission, never on a generic failure.
        Err(e) if e.is_permission_denied() => elevated_delete(&state, &session_id, &path, is_dir).await,
        Err(e) => Err(e.to_string()),
    }
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

    // Recorded before the watch is (re-)armed so the download's own write to the
    // watched directory is recognised as "not an edit" and never uploaded back.
    let synced = tokio::fs::metadata(&local_path).await.ok().as_ref().and_then(FileStamp::of);

    state.watched_files.lock().unwrap().insert(
        local_path.clone(),
        WatchedFile { session_id: session_id.clone(), remote_path: remote_path.clone(), on_event, synced },
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
        files
            .get(path)
            .map(|w| (w.session_id.clone(), w.remote_path.clone(), w.on_event.clone(), w.synced))
    };
    let Some((session_id, remote_path, on_event, synced)) = watched else {
        return;
    };

    let Ok(meta) = tokio::fs::metadata(path).await else {
        return;
    };
    let stamp = FileStamp::of(&meta);

    // Not every event on a watched directory is an edit: our own download writes
    // there, and editors touch attributes. Only sync when the contents moved on.
    if stamp.is_some() && stamp == synced {
        return;
    }

    let _ = on_event.send(FileSyncEvent::Uploading);

    match sync_watched_file(state.inner(), &session_id, path, &remote_path, &meta).await {
        Ok(elevated) => {
            if let Some(watched) = state.watched_files.lock().unwrap().get_mut(path) {
                watched.synced = stamp;
            }
            let _ = on_event.send(FileSyncEvent::Uploaded { elevated });
        }
        Err(message) => {
            let _ = on_event.send(FileSyncEvent::Error { message });
        }
    }
}

// Writes the edited file back, escalating to sudo only if the server refuses the
// plain SFTP write for lack of permission. Returns whether escalation was used.
async fn sync_watched_file(
    state: &AppState,
    session_id: &str,
    local_path: &Path,
    remote_path: &str,
    meta: &std::fs::Metadata,
) -> Result<bool, String> {
    let session = get_sftp(state, session_id).await?;
    match sftp::upload_file(&session, local_path, remote_path, meta, &|_| {}).await {
        Ok(_) => Ok(false),
        Err(e) if e.is_permission_denied() => {
            elevated_write(state, session_id, local_path, remote_path).await.map(|()| true)
        }
        Err(e) => Err(e.to_string()),
    }
}

// Quotes a value for POSIX sh: inside single quotes everything is literal, and an
// embedded quote is closed, escaped and reopened.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::shell_quote;

    // The remote path comes from the server's own directory listing, so it can hold
    // anything a filename may hold — it must never be able to end the quoted string
    // and start a new command.
    #[test]
    fn quotes_paths_hostile_to_a_shell() {
        assert_eq!(shell_quote("/usr/local/bin/x.sh"), "'/usr/local/bin/x.sh'");
        assert_eq!(shell_quote("/srv/my scripts/x.sh"), "'/srv/my scripts/x.sh'");
        assert_eq!(shell_quote("/tmp/$(id).sh"), "'/tmp/$(id).sh'");
        assert_eq!(shell_quote("/tmp/a'; rm -rf /; '.sh"), r"'/tmp/a'\''; rm -rf /; '\''.sh'");
    }
}

// The connection's saved SSH password, to be reused for sudo. Key-based
// connections only have a key passphrase stored, which is not a login password
// and must never be handed to sudo — those get password-less sudo or nothing.
fn sudo_password(state: &AppState, connection_id: &str) -> Option<String> {
    let uuid = Uuid::parse_str(connection_id).ok()?;
    let profile = state.connections.lock().unwrap().get(&uuid)?;
    if !matches!(profile.auth_type, AuthType::Password) {
        return None;
    }
    secrets::get_secret(connection_id, &profile.username, SecretKind::Password).ok().flatten()
}

fn session_ssh(state: &AppState, session_id: &str) -> Result<(Arc<russh::client::Handle<Client>>, String), String> {
    let sessions = state.sessions.lock().unwrap();
    let session = sessions.get(session_id).ok_or_else(|| "session not found".to_string())?;
    Ok((session.ssh.clone(), session.connection_id.clone()))
}

// Runs one already-quoted command line under sudo on the session's connection.
// `args` must have every interpolated path passed through `shell_quote`.
async fn run_with_sudo(state: &AppState, session_id: &str, args: &str) -> Result<(), String> {
    let (ssh, connection_id) = session_ssh(state, session_id)?;
    let password = sudo_password(state, &connection_id);

    // `-S` takes the password from stdin, `-p ''` drops the prompt text; with no
    // password to offer, `-n` fails fast instead of hanging on a hidden prompt.
    //
    // Deliberately free of shell operators: `exec` runs this through the account's
    // *login* shell, and `; rc=$?; exit $rc` is a syntax error under fish or csh.
    // Quoted words alone behave identically everywhere.
    let command = format!("sudo {} {}", if password.is_some() { "-S -p ''" } else { "-n" }, args);

    // sudo -S reads a *line*: without the terminator it sits waiting and then
    // reports that no password was provided.
    let stdin = password.map(|password| format!("{password}\n"));
    let output = ssh::exec::run(&ssh, &command, stdin.as_deref()).await.map_err(|e| e.to_string())?;

    if output.status != 0 {
        return Err(output
            .stderr
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .next_back()
            .unwrap_or("sudo refused the operation")
            .to_string());
    }
    Ok(())
}

// SFTP has no notion of privilege escalation — the subsystem runs as the login
// user, so a root-owned file simply cannot be opened for writing. Write it by
// staging the content somewhere the login user *can* write and copying it into
// place with sudo over an exec channel. `cp` onto an existing path keeps the
// target's inode, owner and mode, which is what editing in place should do
// (`mv` would replace the file with one owned by root and stamped 0600).
async fn elevated_write(
    state: &AppState,
    session_id: &str,
    local_path: &Path,
    remote_path: &str,
) -> Result<(), String> {
    let session = get_sftp(state, session_id).await?;
    let staged = format!("/tmp/.sshmanager-{}", Uuid::new_v4());
    let total_bytes = tokio::fs::metadata(local_path).await.map_err(|e| e.to_string())?.len();
    sftp::copy_to_remote(&session, local_path, &staged, total_bytes, &|_| {})
        .await
        .map_err(|e| format!("could not stage the file for a privileged write: {e}"))?;

    let copied =
        run_with_sudo(state, session_id, &format!("cp -- {} {}", shell_quote(&staged), shell_quote(remote_path)))
            .await;

    // The staging file belongs to the login user, so clearing it needs no privileges
    // and must happen whether or not the copy went through.
    if let Ok((ssh, _)) = session_ssh(state, session_id) {
        let _ = ssh::exec::run(&ssh, &format!("rm -f -- {}", shell_quote(&staged)), None).await;
    }

    copied.map_err(|reason| format!("cannot write {remote_path}, and sudo could not either: {reason}"))
}

// Same reasoning as the privileged write: deleting an entry needs write permission
// on the *directory* holding it, which a root-owned path like /usr/local/bin does
// not give the login user, and no SFTP request can escalate.
async fn elevated_delete(state: &AppState, session_id: &str, path: &str, is_dir: bool) -> Result<(), String> {
    // A `sudo rm -r` is worth one guard: an empty or root path here would take the
    // whole filesystem with it, and no legitimate panel action produces one.
    if path.trim_matches('/').is_empty() {
        return Err("refusing to delete the filesystem root".to_string());
    }

    let args = format!("rm {}-- {}", if is_dir { "-r " } else { "" }, shell_quote(path));
    run_with_sudo(state, session_id, &args)
        .await
        .map_err(|reason| format!("cannot delete {path}, and sudo could not either: {reason}"))
}
