use std::sync::Arc;

use russh_sftp::client::SftpSession;
use serde::Serialize;

use super::{client::Client, SshError};

#[derive(Debug, Clone, Serialize)]
pub struct SftpEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: Option<u64>,
    pub modified: Option<i64>,
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

pub async fn upload(sftp: &SftpSession, local_path: &str, remote_path: &str) -> Result<(), SshError> {
    let data = tokio::fs::read(local_path).await?;
    sftp.write(remote_path, &data).await.map_err(SshError::Sftp)?;
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
