use russh::ChannelMsg;
use serde::Serialize;
use tauri::ipc::Channel as IpcChannel;
use tokio::sync::mpsc;

use crate::storage::ConnectionProfile;

use super::{client, SshError};

pub enum SessionCommand {
    Write(Vec<u8>),
    Resize { cols: u16, rows: u16 },
    Close,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TerminalEvent {
    Data { data: String },
    Closed { code: Option<u32> },
    Error { message: String },
}

pub async fn open(
    profile: ConnectionProfile,
    secret: Option<String>,
    cols: u16,
    rows: u16,
    on_event: IpcChannel<TerminalEvent>,
) -> Result<mpsc::UnboundedSender<SessionCommand>, SshError> {
    let session = client::connect_and_auth(&profile, secret).await?;
    let channel = session.channel_open_session().await.map_err(SshError::Channel)?;
    channel
        .request_pty(false, "xterm-256color", cols as u32, rows as u32, 0, 0, &[])
        .await
        .map_err(SshError::Channel)?;
    channel.request_shell(true).await.map_err(SshError::Channel)?;

    let (tx, mut rx) = mpsc::unbounded_channel::<SessionCommand>();

    tokio::spawn(async move {
        let mut channel = channel;
        loop {
            tokio::select! {
                cmd = rx.recv() => {
                    match cmd {
                        Some(SessionCommand::Write(data)) => {
                            if channel.data_bytes(data).await.is_err() {
                                let _ = on_event.send(TerminalEvent::Error {
                                    message: "connection lost while sending input".to_string(),
                                });
                                break;
                            }
                        }
                        Some(SessionCommand::Resize { cols, rows }) => {
                            let _ = channel.window_change(cols as u32, rows as u32, 0, 0).await;
                        }
                        Some(SessionCommand::Close) | None => {
                            let _ = channel.close().await;
                            break;
                        }
                    }
                }
                msg = channel.wait() => {
                    match msg {
                        Some(ChannelMsg::Data { data }) => {
                            let text = String::from_utf8_lossy(&data).into_owned();
                            if on_event.send(TerminalEvent::Data { data: text }).is_err() {
                                break;
                            }
                        }
                        Some(ChannelMsg::ExtendedData { data, .. }) => {
                            let text = String::from_utf8_lossy(&data).into_owned();
                            let _ = on_event.send(TerminalEvent::Data { data: text });
                        }
                        Some(ChannelMsg::ExitStatus { exit_status }) => {
                            let _ = on_event.send(TerminalEvent::Closed { code: Some(exit_status) });
                            break;
                        }
                        Some(ChannelMsg::Close) | Some(ChannelMsg::Eof) | None => {
                            let _ = on_event.send(TerminalEvent::Closed { code: None });
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }
        let _ = session.disconnect(russh::Disconnect::ByApplication, "", "en").await;
    });

    Ok(tx)
}
