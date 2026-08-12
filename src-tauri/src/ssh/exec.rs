use std::sync::Arc;

use russh::ChannelMsg;

use super::{client::Client, SshError};

pub struct ExecOutput {
    pub status: u32,
    pub stderr: String,
}

// Runs a single command on its own channel over an already-authenticated
// connection — the same `&self` channel-opening trick the SFTP browser uses, so
// no second TCP connection or auth round-trip. `stdin` is written and the write
// side closed before the exit status is collected; it is the only safe place to
// hand a password to a remote command, since anything on the command line is
// visible to every other user via `ps`.
pub async fn run(
    ssh: &Arc<russh::client::Handle<Client>>,
    command: &str,
    stdin: Option<&str>,
) -> Result<ExecOutput, SshError> {
    let mut channel = ssh.channel_open_session().await.map_err(SshError::Channel)?;
    channel.exec(true, command).await.map_err(SshError::Channel)?;

    if let Some(stdin) = stdin {
        channel.data(stdin.as_bytes()).await.map_err(SshError::Channel)?;
    }
    channel.eof().await.map_err(SshError::Channel)?;

    let mut stderr = String::new();
    let mut status = None;
    while let Some(msg) = channel.wait().await {
        match msg {
            ChannelMsg::ExtendedData { ref data, ext } if ext == 1 => {
                stderr.push_str(&String::from_utf8_lossy(data));
            }
            ChannelMsg::ExitStatus { exit_status } => status = Some(exit_status),
            _ => {}
        }
    }

    // No exit status at all means the command never reported one (channel closed
    // early); treat that as a failure rather than a silent success.
    Ok(ExecOutput { status: status.unwrap_or(1), stderr })
}
