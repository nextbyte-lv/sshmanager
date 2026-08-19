use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use russh_sftp::client::fs::Metadata;
use russh_sftp::client::SftpSession;
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::{client::Client, SshError};

const UPLOAD_CHUNK_SIZE: usize = 256 * 1024;

// The mode half of an SFTP `permissions` attribute: three special bits
// (setuid/setgid/sticky) above the owner/group/other r-w-x triads. The bits above
// these hold the file *type*, which chmod cannot change and a setstat must not
// send.
pub const MODE_BITS: u32 = 0o7777;

#[derive(Debug, Clone, Serialize)]
pub struct SftpEntry {
    pub name: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: Option<u64>,
    pub modified: Option<i64>,
    pub mode: Option<u32>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UploadEvent {
    Started { path: String, total_bytes: u64 },
    Progress { path: String, bytes_done: u64, total_bytes: u64 },
    Skipped { path: String },
    FileDone { path: String },
    FileError { path: String, message: String },
    Done { uploaded: u32, skipped: u32, failed: u32 },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FileSyncEvent {
    Uploading,
    Uploaded { elevated: bool },
    Error { message: String },
}

pub async fn open_sftp(ssh: &Arc<russh::client::Handle<Client>>) -> Result<SftpSession, SshError> {
    let channel = ssh.channel_open_session().await.map_err(SshError::Channel)?;
    channel.request_subsystem(true, "sftp").await.map_err(SshError::Channel)?;
    SftpSession::new(channel.into_stream()).await.map_err(SshError::Sftp)
}

pub async fn canonicalize(sftp: &SftpSession, path: &str) -> Result<String, SshError> {
    sftp.canonicalize(path).await.map_err(SshError::Sftp)
}

pub async fn list_dir(sftp: &SftpSession, path: &str) -> Result<Vec<SftpEntry>, SshError> {
    let entries = sftp.read_dir(path).await.map_err(SshError::Sftp)?;
    let mut result: Vec<SftpEntry> = entries
        .map(|entry| {
            let metadata = entry.metadata();
            let file_type = metadata.file_type();
            SftpEntry {
                name: entry.file_name(),
                is_dir: file_type.is_dir(),
                // A directory listing carries lstat attributes, so this is the
                // link's own type — and the mode below is the link's own mode.
                is_symlink: file_type.is_symlink(),
                size: metadata.size,
                modified: metadata.mtime.map(|m| m as i64),
                mode: metadata.permissions.map(|bits| bits & MODE_BITS),
                uid: metadata.uid,
                gid: metadata.gid,
            }
        })
        .collect();
    result.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(result)
}

pub async fn download(sftp: &SftpSession, remote_path: &str, local_path: &str) -> Result<(), SshError> {
    let data = sftp
        .read(remote_path)
        .await
        .map_err(|source| SshError::RemoteRead { path: remote_path.to_string(), source })?;
    tokio::fs::write(local_path, data).await?;
    Ok(())
}

fn join_remote(dir: &str, name: &str) -> String {
    format!("{}/{}", dir.trim_end_matches('/'), name)
}

fn local_mtime_secs(meta: &std::fs::Metadata) -> Option<u32> {
    meta.modified().ok()?.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs() as u32)
}

// Streams a local file's contents onto a remote path, replacing whatever is there.
// Split out from `upload_file` so callers that have already decided to transfer —
// e.g. staging content for a privileged write — can skip the freshness check.
pub(crate) async fn copy_to_remote(
    sftp: &SftpSession,
    local_path: &Path,
    remote_path: &str,
    total_bytes: u64,
    on_event: &impl Fn(UploadEvent),
) -> Result<(), SshError> {
    let mut local_file = tokio::fs::File::open(local_path).await?;
    let mut remote_file = sftp
        .create(remote_path)
        .await
        .map_err(|source| SshError::RemoteWrite { path: remote_path.to_string(), source })?;

    let mut buf = vec![0u8; UPLOAD_CHUNK_SIZE];
    let mut done = 0u64;
    loop {
        let n = local_file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        remote_file.write_all(&buf[..n]).await.map_err(SshError::Transfer)?;
        done += n as u64;
        on_event(UploadEvent::Progress { path: remote_path.to_string(), bytes_done: done, total_bytes });
    }
    remote_file.shutdown().await.map_err(SshError::Transfer)?;
    Ok(())
}

// Uploads a single file, skipping the transfer if a remote file already exists with the
// same size and mtime (rsync's default "quick check"). Returns true if the file was
// actually transferred, false if it was skipped.
pub(crate) async fn upload_file(
    sftp: &SftpSession,
    local_path: &Path,
    remote_path: &str,
    local_meta: &std::fs::Metadata,
    on_event: &impl Fn(UploadEvent),
) -> Result<bool, SshError> {
    let local_size = local_meta.len();
    let local_mtime = local_mtime_secs(local_meta);

    if let Some(local_mtime) = local_mtime {
        if let Ok(remote_meta) = sftp.metadata(remote_path).await {
            if remote_meta.size == Some(local_size) && remote_meta.mtime == Some(local_mtime) {
                on_event(UploadEvent::Skipped { path: remote_path.to_string() });
                return Ok(false);
            }
        }
    }

    on_event(UploadEvent::Started { path: remote_path.to_string(), total_bytes: local_size });
    copy_to_remote(sftp, local_path, remote_path, local_size, on_event).await?;

    if let Some(local_mtime) = local_mtime {
        let mut attrs = Metadata::empty();
        attrs.mtime = Some(local_mtime);
        attrs.atime = Some(local_mtime);
        let _ = sftp.set_metadata(remote_path, attrs).await;
    }

    on_event(UploadEvent::FileDone { path: remote_path.to_string() });
    Ok(true)
}

// Uploads a local file or directory tree to the remote path, recursing into
// subdirectories and skipping files that are already identical on the remote.
pub async fn upload_path(
    sftp: &SftpSession,
    local_root: &Path,
    remote_root: &str,
    on_event: &impl Fn(UploadEvent),
) -> Result<(), SshError> {
    let mut stack: Vec<(PathBuf, String)> = vec![(local_root.to_path_buf(), remote_root.to_string())];
    let mut uploaded = 0u32;
    let mut skipped = 0u32;
    let mut failed = 0u32;

    while let Some((local, remote)) = stack.pop() {
        let meta = match tokio::fs::metadata(&local).await {
            Ok(meta) => meta,
            Err(e) => {
                failed += 1;
                on_event(UploadEvent::FileError { path: remote, message: e.to_string() });
                continue;
            }
        };

        if meta.is_dir() {
            if sftp.create_dir(&remote).await.is_err() {
                match sftp.metadata(&remote).await {
                    Ok(remote_meta) if remote_meta.is_dir() => {}
                    _ => {
                        failed += 1;
                        on_event(UploadEvent::FileError {
                            path: remote,
                            message: "failed to create remote directory".to_string(),
                        });
                        continue;
                    }
                }
            }

            let mut entries = match tokio::fs::read_dir(&local).await {
                Ok(entries) => entries,
                Err(e) => {
                    failed += 1;
                    on_event(UploadEvent::FileError { path: remote, message: e.to_string() });
                    continue;
                }
            };
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name().to_string_lossy().into_owned();
                stack.push((entry.path(), join_remote(&remote, &name)));
            }
        } else {
            match upload_file(sftp, &local, &remote, &meta, on_event).await {
                Ok(true) => uploaded += 1,
                Ok(false) => skipped += 1,
                Err(e) => {
                    failed += 1;
                    on_event(UploadEvent::FileError { path: remote, message: e.to_string() });
                }
            }
        }
    }

    on_event(UploadEvent::Done { uploaded, skipped, failed });
    Ok(())
}

pub async fn make_dir(sftp: &SftpSession, path: &str) -> Result<(), SshError> {
    sftp.create_dir(path).await.map_err(SshError::Sftp)
}

pub async fn remove(sftp: &SftpSession, path: &str, is_dir: bool) -> Result<(), SshError> {
    if !is_dir {
        return remove_file(sftp, path).await;
    }
    remove_dir_all(sftp, path).await
}

async fn remove_file(sftp: &SftpSession, path: &str) -> Result<(), SshError> {
    sftp.remove_file(path)
        .await
        .map_err(|source| SshError::RemoteDelete { path: path.to_string(), source })
}

// SFTP's RMDIR removes only *empty* directories — there is no recursive delete in
// the protocol — so the tree has to be walked and emptied first. Collects
// directories breadth-first while deleting the files it passes, then removes the
// directories in reverse discovery order, which guarantees children go before the
// parent that holds them. Iterative rather than recursive to keep the future sized
// (an async fn cannot recurse without boxing).
async fn remove_dir_all(sftp: &SftpSession, root: &str) -> Result<(), SshError> {
    let mut dirs = vec![root.to_string()];
    let mut next = 0;

    while next < dirs.len() {
        let dir = dirs[next].clone();
        next += 1;

        for entry in list_dir(sftp, &dir).await? {
            // `read_dir` filters these, but nothing this destructive should depend
            // on that: recursing into `..` would walk *up* and delete the parent.
            if entry.name == "." || entry.name == ".." {
                continue;
            }
            let child = join_remote(&dir, &entry.name);
            // A symlink reports as a file here (directory listings carry lstat
            // attributes), so links are unlinked rather than followed and emptied.
            if entry.is_dir {
                dirs.push(child);
            } else {
                remove_file(sftp, &child).await?;
            }
        }
    }

    for dir in dirs.into_iter().rev() {
        sftp.remove_dir(&dir).await.map_err(|source| SshError::RemoteDelete { path: dir, source })?;
    }
    Ok(())
}

pub async fn rename(sftp: &SftpSession, from: &str, to: &str) -> Result<(), SshError> {
    sftp.rename(from, to).await.map_err(SshError::Sftp)
}

// Changes one entry's mode. Only the mode bits are sent: a setstat whose
// `permissions` field still carried the type bits read back from a listing would
// be asking the server to chmod a file into a different kind of file. OpenSSH
// masks the value before calling chmod(2), but nothing in the protocol says a
// server must.
pub async fn set_mode(sftp: &SftpSession, path: &str, mode: u32) -> Result<(), SshError> {
    let mut attrs = Metadata::empty();
    attrs.permissions = Some(mode & MODE_BITS);
    sftp.set_metadata(path, attrs)
        .await
        .map_err(|source| SshError::RemoteChmod { path: path.to_string(), source })
}

// Applies one mode to a whole subtree, like `chmod -R`. SFTP has no recursive
// setstat, so the tree is walked the same way the recursive delete walks it.
//
// The walk is completed *before* anything is chmod-ed, and the modes are then
// applied to files first and to directories deepest-first. Applying as we
// discover would let the operation lock itself out — a mode without owner
// execute (0644, 0600) on a directory we still have to descend into makes the
// next `read_dir` fail — and the same ordering means a half-finished run never
// leaves an unreachable directory behind.
//
// Symlinks are skipped: a setstat follows the link, so chmod-ing one would
// silently re-mode whatever it points at, possibly outside this tree. `chmod -R`
// leaves them alone for the same reason.
pub async fn set_mode_recursive(sftp: &SftpSession, root: &str, mode: u32) -> Result<(), SshError> {
    let mut dirs = vec![root.to_string()];
    let mut files: Vec<String> = Vec::new();
    let mut next = 0;

    while next < dirs.len() {
        let dir = dirs[next].clone();
        next += 1;

        for entry in list_dir(sftp, &dir).await? {
            if entry.name == "." || entry.name == ".." || entry.is_symlink {
                continue;
            }
            let child = join_remote(&dir, &entry.name);
            if entry.is_dir {
                dirs.push(child);
            } else {
                files.push(child);
            }
        }
    }

    for file in &files {
        set_mode(sftp, file, mode).await?;
    }
    for dir in dirs.iter().rev() {
        set_mode(sftp, dir, mode).await?;
    }
    Ok(())
}
