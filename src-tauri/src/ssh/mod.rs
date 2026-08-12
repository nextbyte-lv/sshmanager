pub mod client;
pub mod exec;
pub mod pty;
pub mod sftp;

// russh_sftp renders a status packet as "<code>: <message>", and servers commonly
// send the code's own text as the message, giving "Permission denied: Permission
// denied". Collapse that duplication before it reaches the UI.
fn sftp_reason(error: &russh_sftp::client::error::Error) -> String {
    let text = error.to_string();
    match text.split_once(": ") {
        Some((code, message)) if code.eq_ignore_ascii_case(message) => code.to_string(),
        _ => text,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SshError {
    #[error("failed to connect: {0}")]
    Connect(russh::Error),
    #[error("authentication error: {0}")]
    Auth(russh::Error),
    #[error("authentication failed")]
    AuthFailed,
    #[error("channel error: {0}")]
    Channel(russh::Error),
    #[error("failed to load private key: {0}")]
    KeyLoad(russh::keys::Error),
    #[error("this connection has no password saved")]
    MissingPassword,
    #[error("this connection has no private key file configured")]
    MissingKeyPath,
    #[error("sftp error: {}", sftp_reason(.0))]
    Sftp(russh_sftp::client::error::Error),
    #[error("cannot read {path} on the server: {}", sftp_reason(source))]
    RemoteRead { path: String, source: russh_sftp::client::error::Error },
    #[error("cannot write {path} on the server: {}", sftp_reason(source))]
    RemoteWrite { path: String, source: russh_sftp::client::error::Error },
    #[error("cannot delete {path} on the server: {}", sftp_reason(source))]
    RemoteDelete { path: String, source: russh_sftp::client::error::Error },
    #[error("local file error: {0}")]
    LocalIo(#[from] std::io::Error),
    #[error("file transfer error: {0}")]
    Transfer(std::io::Error),
}

impl SshError {
    // True only when the server refused an operation for lack of permission — the
    // one failure a privileged retry could plausibly get past.
    pub fn is_permission_denied(&self) -> bool {
        let source = match self {
            Self::Sftp(source)
            | Self::RemoteRead { source, .. }
            | Self::RemoteWrite { source, .. }
            | Self::RemoteDelete { source, .. } => source,
            _ => return false,
        };
        matches!(
            source,
            russh_sftp::client::error::Error::Status(status)
                if status.status_code == russh_sftp::protocol::StatusCode::PermissionDenied
        )
    }
}
