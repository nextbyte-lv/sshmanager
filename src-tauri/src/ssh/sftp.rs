use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use russh_sftp::client::fs::Metadata;
use russh_sftp::client::SftpSession;
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::{client::Client, SshError};

const UPLOAD_CHUNK_SIZE: usize = 256 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct SftpEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: Option<u64>,
    pub modified: Option<i64>,
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
    Uploaded,
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
            SftpEntry {
                name: entry.file_name(),
                is_dir: metadata.file_type().is_dir(),
                size: metadata.size,
                modified: metadata.mtime.map(|m| m as i64),
            }
        })
        .collect();
    result.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(result)
}

pub async fn download(sftp: &SftpSession, remote_path: &str, local_path: &str) -> Result<(), SshError> {
    let data = sftp.read(remote_path).await.map_err(SshError::Sftp)?;
    tokio::fs::write(local_path, data).await?;
    Ok(())
}

fn join_remote(dir: &str, name: &str) -> String {
    format!("{}/{}", dir.trim_end_matches('/'), name)
}

fn local_mtime_secs(meta: &std::fs::Metadata) -> Option<u32> {
    meta.modified().ok()?.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs() as u32)
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

    let mut local_file = tokio::fs::File::open(local_path).await?;
    let mut remote_file = sftp.create(remote_path).await.map_err(SshError::Sftp)?;

    let mut buf = vec![0u8; UPLOAD_CHUNK_SIZE];
    let mut done = 0u64;
    loop {
        let n = local_file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        remote_file.write_all(&buf[..n]).await.map_err(SshError::Transfer)?;
        done += n as u64;
        on_event(UploadEvent::Progress { path: remote_path.to_string(), bytes_done: done, total_bytes: local_size });
    }
    remote_file.shutdown().await.map_err(SshError::Transfer)?;

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
    if is_dir {
        sftp.remove_dir(path).await.map_err(SshError::Sftp)
    } else {
        sftp.remove_file(path).await.map_err(SshError::Sftp)
    }
}

pub async fn rename(sftp: &SftpSession, from: &str, to: &str) -> Result<(), SshError> {
    sftp.rename(from, to).await.map_err(SshError::Sftp)
}
