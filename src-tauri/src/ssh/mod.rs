pub mod client;
pub mod pty;

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
}
