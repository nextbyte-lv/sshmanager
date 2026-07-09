use std::sync::Arc;
use std::time::Duration;

use russh::{Channel, ChannelMsg};
use serde::Serialize;
use tauri::ipc::Channel as IpcChannel;
use tauri::{AppHandle, Manager};
use tokio::sync::mpsc;
use tokio::time::sleep;

use crate::state::AppState;
use crate::storage::ConnectionProfile;

use super::{client, SshError};

const MAX_RECONNECT_ATTEMPTS: u32 = 5;
const RECONNECT_DELAY: Duration = Duration::from_secs(2);

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
    Reconnecting { attempt: u32, max_attempts: u32 },
    Reconnected,
}

type Established = (Arc<russh::client::Handle<client::Client>>, Channel<russh::client::Msg>);

async fn establish(
    profile: &ConnectionProfile,
    secret: Option<String>,
    cols: u16,
    rows: u16,
) -> Result<Established, SshError> {
    let session = Arc::new(client::connect_and_auth(profile, secret).await?);
    let channel = session.channel_open_session().await.map_err(SshError::Channel)?;
    channel
        .request_pty(false, "xterm-256color", cols as u32, rows as u32, 0, 0, &[])
        .await
        .map_err(SshError::Channel)?;
    channel.request_shell(true).await.map_err(SshError::Channel)?;
    Ok((session, channel))
}

pub async fn open(
    app: AppHandle,
    session_id: String,
    profile: ConnectionProfile,
    secret: Option<String>,
    cols: u16,
    rows: u16,
    on_event: IpcChannel<TerminalEvent>,
) -> Result<(mpsc::UnboundedSender<SessionCommand>, Arc<russh::client::Handle<client::Client>>), SshError> {
    let (session, channel) = establish(&profile, secret.clone(), cols, rows).await?;

    let (tx, mut rx) = mpsc::unbounded_channel::<SessionCommand>();
    let task_session = session.clone();

    tokio::spawn(async move {
        let mut session = task_session;
        let mut channel = channel;
        let mut cols = cols;
        let mut rows = rows;

        'connection: loop {
            // This inner loop only exits via a plain `break` when the connection has
            // dropped out from under us (write failure, or the channel ending without an
            // exit status) — every other end-of-session path breaks 'connection directly,
            // skipping the reconnect attempt below entirely.
            loop {
                tokio::select! {
                    cmd = rx.recv() => {
                        match cmd {
                            Some(SessionCommand::Write(data)) => {
                                if channel.data_bytes(data).await.is_err() {
                                    break;
                                }
                            }
                            Some(SessionCommand::Resize { cols: new_cols, rows: new_rows }) => {
                                cols = new_cols;
                                rows = new_rows;
                                let _ = channel.window_change(cols as u32, rows as u32, 0, 0).await;
                            }
                            Some(SessionCommand::Close) | None => {
                                let _ = channel.close().await;
                                break 'connection;
                            }
                        }
                    }
                    msg = channel.wait() => {
                        match msg {
                            Some(ChannelMsg::Data { data }) => {
                                let text = String::from_utf8_lossy(&data).into_owned();
                                if on_event.send(TerminalEvent::Data { data: text }).is_err() {
                                    break 'connection;
                                }
                            }
                            Some(ChannelMsg::ExtendedData { data, .. }) => {
                                let text = String::from_utf8_lossy(&data).into_owned();
                                let _ = on_event.send(TerminalEvent::Data { data: text });
                            }
                            Some(ChannelMsg::ExitStatus { exit_status }) => {
                                let _ = on_event.send(TerminalEvent::Closed { code: Some(exit_status) });
                                break 'connection;
                            }
                            Some(ChannelMsg::Close) | Some(ChannelMsg::Eof) | None => {
                                break;
                            }
                            _ => {}
                        }
                    }
                }
            }

            let _ = session.disconnect(russh::Disconnect::ByApplication, "", "en").await;

            let mut reconnected = None;
            'retry: for attempt in 1..=MAX_RECONNECT_ATTEMPTS {
                let _ = on_event.send(TerminalEvent::Reconnecting { attempt, max_attempts: MAX_RECONNECT_ATTEMPTS });

                match establish(&profile, secret.clone(), cols, rows).await {
                    Ok(pair) => {
                        reconnected = Some(pair);
                        break 'retry;
                    }
                    Err(_) if attempt < MAX_RECONNECT_ATTEMPTS => {
                        // Wait out the backoff, but let an explicit close cut it short. Any
                        // other command received during backoff (typed input, a resize) is
                        // discarded rather than replayed after reconnect.
                        tokio::select! {
                            _ = sleep(RECONNECT_DELAY) => {}
                            _ = async {
                                loop {
                                    match rx.recv().await {
                                        Some(SessionCommand::Close) | None => break,
                                        _ => continue,
                                    }
                                }
                            } => {
                                break 'connection;
                            }
                        }
                    }
                    Err(_) => {}
                }
            }

            match reconnected {
                Some((new_session, new_channel)) => {
                    if let Some(state) = app.try_state::<AppState>() {
                        if let Some(handle) = state.sessions.lock().unwrap().get_mut(&session_id) {
                            handle.ssh = new_session.clone();
                        }
                        state.sftp.lock().unwrap().remove(&session_id);
                    }
                    session = new_session;
                    channel = new_channel;
                    let _ = on_event.send(TerminalEvent::Reconnected);
                }
                None => {
                    let _ = on_event.send(TerminalEvent::Error {
                        message: format!("connection lost, reconnect failed after {MAX_RECONNECT_ATTEMPTS} attempts"),
                    });
                    if let Some(state) = app.try_state::<AppState>() {
                        state.sessions.lock().unwrap().remove(&session_id);
                        state.sftp.lock().unwrap().remove(&session_id);
                    }
                    break 'connection;
                }
            }
        }

        let _ = session.disconnect(russh::Disconnect::ByApplication, "", "en").await;
    });

    Ok((tx, session))
}
