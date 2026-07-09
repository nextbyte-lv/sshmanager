pub mod client;
pub mod pty;
pub mod sftp;

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
    #[error("sftp error: {0}")]
    Sftp(russh_sftp::client::error::Error),
    #[error("local file error: {0}")]
    LocalIo(#[from] std::io::Error),
    #[error("file transfer error: {0}")]
    Transfer(std::io::Error),
}
