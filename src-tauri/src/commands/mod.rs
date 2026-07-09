pub mod connections;
pub mod session;

use crate::secrets::SecretKind;
use crate::storage::AuthType;

pub(crate) fn secret_kind_for(auth_type: AuthType) -> SecretKind {
    match auth_type {
        AuthType::Password => SecretKind::Password,
        AuthType::Key => SecretKind::Passphrase,
    }
}
