use std::sync::Arc;

use russh::client;
use russh::keys::{load_secret_key, PrivateKeyWithHashAlg};

use crate::storage::{AuthType, ConnectionProfile};

use super::SshError;

pub struct Client;

impl client::Handler for Client {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        // Personal single-user tool: trust on first connect, no host-key store yet.
        Ok(true)
    }
}

pub async fn connect_and_auth(
    profile: &ConnectionProfile,
    secret: Option<String>,
) -> Result<client::Handle<Client>, SshError> {
    let config = Arc::new(client::Config::default());
    let mut session = client::connect(config, (profile.host.as_str(), profile.port), Client)
        .await
        .map_err(SshError::Connect)?;

    let auth_result = match profile.auth_type {
        AuthType::Password => {
            let password = secret.ok_or(SshError::MissingPassword)?;
            session
                .authenticate_password(profile.username.clone(), password)
                .await
                .map_err(SshError::Auth)?
        }
        AuthType::Key => {
            let key_path = profile.key_path.clone().ok_or(SshError::MissingKeyPath)?;
            let key_pair = load_secret_key(key_path, secret.as_deref()).map_err(SshError::KeyLoad)?;
            let hash_alg = session.best_supported_rsa_hash().await.map_err(SshError::Auth)?.flatten();
            session
                .authenticate_publickey(
                    profile.username.clone(),
                    PrivateKeyWithHashAlg::new(Arc::new(key_pair), hash_alg),
                )
                .await
                .map_err(SshError::Auth)?
        }
    };

    if !auth_result.success() {
        return Err(SshError::AuthFailed);
    }

    Ok(session)
}
