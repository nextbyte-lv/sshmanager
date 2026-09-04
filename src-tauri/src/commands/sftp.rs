use std::path::{Path, PathBuf};
use std::sync::Arc;

use russh_sftp::client::SftpSession;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

use crate::secrets::{self, SecretKind};
use crate::ssh::client::Client;
use crate::ssh::sftp::{self, DirSize, FileSyncEvent, SftpEntry, UploadEvent};
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

// Recursive sizes for the given directories. Deliberately a separate, explicit
// call rather than part of `sftp_list_dir`: SFTP has no directory-size attribute,
// so every one of these is a full walk of the tree, and doing that eagerly for a
// listing of `/` would stat the whole machine before the panel could paint.
//
// The walk runs on the remote as a single `du`, not as a recursive `read_dir` from
// here — one round trip and the server's own directory cache, instead of one
// request per directory across the wire.
#[tauri::command]
pub async fn sftp_dir_sizes(
    state: State<'_, AppState>,
    session_id: String,
    paths: Vec<String>,
) -> Result<Vec<DirSize>, String> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let (ssh, _) = session_ssh(&state, &session_id)?;

    // GNU `du -b` counts apparent bytes, which is the same thing the panel shows
    // for files. BSD, macOS and older busybox `du` reject `-b`, so those fall back
    // to POSIX `-k` and its 1024-byte blocks. Emptiness of stdout picks the
    // fallback, not the exit status: a run that merely hit an unreadable
    // subdirectory also exits nonzero, yet still prints usable totals.
    let apparent = ssh::exec::run(&ssh, &du_command(&paths, true), None).await.map_err(|e| e.to_string())?;
    if !apparent.stdout.trim().is_empty() {
        return Ok(parse_du(&apparent.stdout, &apparent.stderr, 1, &paths));
    }

    let blocks = ssh::exec::run(&ssh, &du_command(&paths, false), None).await.map_err(|e| e.to_string())?;
    if !blocks.stdout.trim().is_empty() {
        return Ok(parse_du(&blocks.stdout, &blocks.stderr, 1024, &paths));
    }

    Err(last_error_line(&blocks.stderr)
        .or_else(|| last_error_line(&apparent.stderr))
        .unwrap_or("the server reported no folder sizes")
        .to_string())
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
    let emit = |event: UploadEvent| {
        let _ = on_event.send(event);
    };
    // Everything the walk cannot do as the login user comes back through here.
    // Captured by reference and by `move` so the closure hands the async block
    // nothing but copies of references — the alternative is a borrow puzzle for
    // no gain, since all three outlive the upload.
    let (state_ref, sid, emit_ref) = (state.inner(), session_id.as_str(), &emit);
    let elevate = move |what: sftp::Elevate| async move {
        match what {
            sftp::Elevate::MakeDir { remote } => elevated_mkdir(state_ref, sid, &remote).await,
            sftp::Elevate::PutFile { local, remote } => {
                elevated_put(state_ref, sid, &local, &remote, emit_ref).await
            }
        }
    };
    sftp::upload_path(&session, Path::new(&local_path), &remote_path, &emit, &elevate)
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
pub async fn sftp_set_mode(
    state: State<'_, AppState>,
    session_id: String,
    path: String,
    mode: u32,
    recursive: bool,
) -> Result<(), String> {
    // The frontend builds the mode from a checkbox grid and an octal field, so it
    // can only ever be a real mode — but this is the boundary, and a value with
    // type bits in it would ask the server to chmod a file into another kind.
    if mode & !sftp::MODE_BITS != 0 {
        return Err(format!("{mode:o} is not a valid file mode"));
    }
    // The same guard the recursive delete carries: a recursive chmod anchored at
    // the filesystem root would re-mode the whole machine, and no panel action
    // legitimately asks for that.
    if recursive && path.trim_matches('/').is_empty() {
        return Err("refusing to change permissions of the whole filesystem".to_string());
    }

    let session = get_sftp(&state, &session_id).await?;
    let result = if recursive {
        sftp::set_mode_recursive(&session, &path, mode).await
    } else {
        sftp::set_mode(&session, &path, mode).await
    };

    match result {
        Ok(()) => Ok(()),
        // Same escalation rule as the write and delete paths: retry under sudo only
        // when the server refused for lack of permission. A recursive run may have
        // re-moded part of the tree before being refused; re-applying the same mode
        // to all of it under sudo is idempotent.
        Err(e) if e.is_permission_denied() => elevated_chmod(&state, &session_id, &path, mode, recursive).await,
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

// `chmod [-R] MODE -- PATH`: the mode is a positional argument, so `--` goes after
// it and only guards the path. Four octal digits keep the special bits, and a
// leading zero leaves no doubt the value is octal.
fn chmod_args(mode: u32, recursive: bool, path: &str) -> String {
    format!("chmod {}{:04o} -- {}", if recursive { "-R " } else { "" }, mode, shell_quote(path))
}

// `mkdir -p -- PATH` and `cp -- SOURCE DEST`. The `--` is the point of both: a
// remote path is user data and one starting with `-` would otherwise be read as a
// flag. `-p` makes the mkdir idempotent, so a directory another entry of the same
// upload already created is not an error.
fn mkdir_args(path: &str) -> String {
    format!("mkdir -p -- {}", shell_quote(path))
}

fn cp_args(source: &str, dest: &str) -> String {
    format!("cp -- {} {}", shell_quote(source), shell_quote(dest))
}

// `du -s<unit> -- PATH...`: `-s` collapses each argument to a single total line,
// and the unit is `b` (apparent bytes, GNU) or `k` (1024-byte blocks, POSIX).
//
// Run through `env` so the C locale is forced without a `VAR=value cmd` prefix,
// which the account's *login* shell may not accept (csh and fish both reject it,
// the same trap the sudo path documents). The locale matters because GNU
// coreutils quotes paths in its diagnostics the way the locale asks it to —
// ‘/root’ in a UTF-8 locale, '/root' under C — and `names_path` reads those
// diagnostics to decide whether a total is complete.
fn du_command(paths: &[String], apparent: bool) -> String {
    let quoted: Vec<String> = paths.iter().map(|path| shell_quote(path)).collect();
    format!("env LC_ALL=C du -s{} -- {}", if apparent { 'b' } else { 'k' }, quoted.join(" "))
}

// `du -s` writes one `<size>\t<path>` line per argument, echoing the path exactly
// as it was given. Results are matched back by that echoed path rather than by
// line order, because an argument `du` cannot stat at all prints no line and would
// otherwise shift every result after it. Only requested paths are returned, so a
// surprising echo cannot invent an entry; a name containing a tab or a newline is
// the one case this can't recover, and that folder is simply left unsized.
fn parse_du(stdout: &str, stderr: &str, unit_bytes: u64, paths: &[String]) -> Vec<DirSize> {
    let sizes: std::collections::HashMap<&str, u64> = stdout
        .lines()
        .filter_map(|line| {
            let (size, path) = line.split_once('\t')?;
            Some((path.trim_end_matches('\r'), size.trim().parse::<u64>().ok()?))
        })
        .collect();

    paths
        .iter()
        .filter_map(|path| {
            let units = sizes.get(path.as_str())?;
            Some(DirSize {
                path: path.clone(),
                bytes: units.saturating_mul(unit_bytes),
                partial: names_path(stderr, path),
            })
        })
        .collect()
}

// `du`'s exit status covers the whole run, so a batched call can't learn from it
// which of its arguments it failed to read. The diagnostics do name the path —
// `du: cannot read directory '/srv/log/private': Permission denied` — so a total
// counts as partial when some diagnostic names that argument or something beneath
// it. Matching `'<path>'` and `<path>/` rather than a bare substring keeps
// `/srv/a` from being blamed for a failure under `/srv/ab`.
fn names_path(stderr: &str, path: &str) -> bool {
    let exact = format!("'{path}'");
    let under = format!("{}/", path.trim_end_matches('/'));
    stderr.lines().any(|line| line.contains(&exact) || line.contains(&under))
}

// The last non-empty stderr line. Both a refusal from `sudo` and a shell's reason
// for never running a command at all (`du: not found`) end up there, and later
// lines are the specific ones — earlier output is usually context leading up to it.
fn last_error_line(stderr: &str) -> Option<&str> {
    stderr.lines().map(str::trim).rfind(|line| !line.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{chmod_args, cp_args, du_command, mkdir_args, names_path, parse_du, shell_quote};

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

    // The mode reaches the shell as text, where `644` vs `0644` vs a dropped
    // special bit are three different outcomes on the file.
    #[test]
    fn renders_the_escalated_chmod_as_octal() {
        assert_eq!(chmod_args(0o644, false, "/etc/hosts"), "chmod 0644 -- '/etc/hosts'");
        assert_eq!(chmod_args(0o755, true, "/srv/site"), "chmod -R 0755 -- '/srv/site'");
        assert_eq!(chmod_args(0o1777, false, "/tmp"), "chmod 1777 -- '/tmp'");
        assert_eq!(chmod_args(0o4755, false, "/usr/bin/x"), "chmod 4755 -- '/usr/bin/x'");
        assert_eq!(chmod_args(0, false, "/tmp/locked"), "chmod 0000 -- '/tmp/locked'");
    }
    // With no operands at all `du` would silently size the working directory, so the
    // command must never be built from an empty list (the caller returns early).
    #[test]
    fn builds_a_batched_du_per_unit() {
        let paths = vec!["/srv/site".to_string(), "/srv/my logs".to_string()];
        assert_eq!(du_command(&paths, true), "env LC_ALL=C du -sb -- '/srv/site' '/srv/my logs'");
        assert_eq!(du_command(&paths, false), "env LC_ALL=C du -sk -- '/srv/site' '/srv/my logs'");
    }

    // The whole point of keying on the echoed path: `/var/empty` prints nothing at
    // all, and a line-order mapping would hand its follower's size to it.
    #[test]
    fn matches_du_output_to_the_path_it_names() {
        let paths = vec!["/a".to_string(), "/var/empty".to_string(), "/b".to_string()];
        let stdout = "4096\t/a\n81920\t/b\n";
        let sizes = parse_du(stdout, "", 1, &paths);
        assert_eq!(sizes.len(), 2);
        assert_eq!((sizes[0].path.as_str(), sizes[0].bytes), ("/a", 4096));
        assert_eq!((sizes[1].path.as_str(), sizes[1].bytes), ("/b", 81920));
    }

    // `-k` reports 1024-byte blocks, so the fallback's numbers are only right after
    // scaling; unrequested and unparsable lines must not become entries.
    #[test]
    fn scales_block_counts_and_ignores_noise() {
        let paths = vec!["/srv".to_string()];
        let stdout = "12\t/srv\n99\t/not-requested\nrubbish\n";
        let sizes = parse_du(stdout, "", 1024, &paths);
        assert_eq!(sizes.len(), 1);
        assert_eq!(sizes[0].bytes, 12 * 1024);
    }

    // Both escalated upload commands must keep `--` in front of the paths: a
    // remote entry named `-rf` is a legal filename and an illegal flag.
    #[test]
    fn guards_the_escalated_upload_paths_against_flags() {
        assert_eq!(mkdir_args("/opt/app/-p"), "mkdir -p -- '/opt/app/-p'");
        assert_eq!(
            cp_args("/tmp/.sshmanager-1", "/etc/app/-rf conf"),
            "cp -- '/tmp/.sshmanager-1' '/etc/app/-rf conf'"
        );
    }

    // A total summed over a tree `du` could not fully read is a lower bound, and
    // has to say so rather than pass for the real size.
    #[test]
    fn flags_the_total_whose_tree_was_unreadable() {
        let stderr = "du: cannot read directory '/srv/ab/private': Permission denied\n";
        assert!(names_path(stderr, "/srv/ab"));
        // A prefix of another argument's path is not the same argument.
        assert!(!names_path(stderr, "/srv/a"));
        // The argument itself being unreadable is named without a trailing slash.
        assert!(names_path("du: cannot read directory '/root': Permission denied", "/root"));
        assert!(!names_path("", "/srv/ab"));
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
        return Err(last_error_line(&output.stderr).unwrap_or("sudo refused the operation").to_string());
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

    let copied = run_with_sudo(state, session_id, &cp_args(&staged, remote_path)).await;

    // The staging file belongs to the login user, so clearing it needs no privileges
    // and must happen whether or not the copy went through.
    if let Ok((ssh, _)) = session_ssh(state, session_id) {
        let _ = ssh::exec::run(&ssh, &format!("rm -f -- {}", shell_quote(&staged)), None).await;
    }

    copied.map_err(|reason| format!("cannot write {remote_path}, and sudo could not either: {reason}"))
}

// A directory the login user cannot create because it cannot write the parent —
// the usual reason a folder upload stops at its first entry. `-p` keeps it
// idempotent, matching the plain SFTP branch's "already a directory is fine" rule.
async fn elevated_mkdir(state: &AppState, session_id: &str, remote: &str) -> Result<(), String> {
    run_with_sudo(state, session_id, &mkdir_args(remote))
        .await
        .map_err(|reason| format!("cannot create {remote} on the server, and sudo could not either: {reason}"))
}

// The upload's counterpart to `elevated_write`: the same staging trick, but it
// reports progress so the panel's bar keeps moving through a large privileged
// file, and it is reached for a file that does not exist yet as often as for one
// that does.
//
// Plain `cp` for the same reason as the write path — onto an existing target it
// keeps that file's inode, owner and mode, where `mv` would replace it with a
// root-owned 0600 file. The cost is the mtime, which ends up as *now* instead of
// the local file's: an escalated file is therefore re-sent on the next upload
// rather than skipped by the size+mtime check. Copying the timestamp needs either
// `cp -p` (which would also hand the target's ownership to the login user) or a
// GNU-only `--preserve=timestamps`, and neither is worth it to save one transfer.
async fn elevated_put(
    state: &AppState,
    session_id: &str,
    local_path: &Path,
    remote_path: &str,
    on_event: &impl Fn(UploadEvent),
) -> Result<(), String> {
    let session = get_sftp(state, session_id).await?;
    let staged = format!("/tmp/.sshmanager-{}", Uuid::new_v4());
    let total_bytes = tokio::fs::metadata(local_path).await.map_err(|e| e.to_string())?.len();

    // Progress is re-labelled with the destination: the staging path is an
    // implementation detail of the escalation, and the panel is showing the
    // upload the user actually asked for.
    let staged_result = sftp::copy_to_remote(&session, local_path, &staged, total_bytes, &|event| {
        on_event(match event {
            UploadEvent::Progress { bytes_done, total_bytes, .. } => {
                UploadEvent::Progress { path: remote_path.to_string(), bytes_done, total_bytes }
            }
            other => other,
        })
    })
    .await;

    let outcome = match staged_result {
        Err(e) => Err(format!("could not stage {remote_path} for a privileged upload: {e}")),
        Ok(()) => run_with_sudo(state, session_id, &cp_args(&staged, remote_path))
            .await
            .map_err(|reason| {
                format!("cannot write {remote_path} on the server, and sudo could not either: {reason}")
            }),
    };

    // The staging file belongs to the login user, so clearing it needs no
    // privileges and must happen whether or not the copy went through.
    if let Ok((ssh, _)) = session_ssh(state, session_id) {
        let _ = ssh::exec::run(&ssh, &format!("rm -f -- {}", shell_quote(&staged)), None).await;
    }

    outcome
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

// And again for the mode: chmod(2) is refused for anyone who is not the file's
// owner (or root), so a file the login user can read and even edit through the
// sudo write path above still can't be re-moded over SFTP. The mode is re-issued
// as a plain `chmod` on an exec channel, printed as four octal digits so the
// special bits survive.
async fn elevated_chmod(
    state: &AppState,
    session_id: &str,
    path: &str,
    mode: u32,
    recursive: bool,
) -> Result<(), String> {
    run_with_sudo(state, session_id, &chmod_args(mode, recursive, path))
        .await
        .map_err(|reason| format!("cannot change permissions of {path}, and sudo could not either: {reason}"))
}
