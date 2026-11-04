use std::sync::Arc;
use std::time::{Duration, Instant};

use russh::{Channel, ChannelMsg};
use serde::Serialize;
use tauri::ipc::Channel as IpcChannel;
use tauri::{AppHandle, Manager};
use tokio::sync::mpsc;
use tokio::time::sleep;

use crate::state::AppState;
use crate::storage::ConnectionProfile;

use super::{client, SshError};

const MAX_RECONNECT_ATTEMPTS: u32 = 10;
const RECONNECT_DELAY: Duration = Duration::from_secs(3);

pub enum SessionCommand {
    Write(Vec<u8>),
    /// Type `line` into the shell, then answer one sudo prompt with `autofill`'s
    /// password if (and only if) its exact prompt shows up in the output.
    WriteWithSudo { line: Vec<u8>, autofill: SudoAutofill },
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

// Long enough for a slow login shell to start the command; short enough that a
// sudo which never prompted (NOPASSWD, a cached timestamp) does not leave the
// password armed for an unrelated prompt much later.
const SUDO_AUTOFILL_WINDOW: Duration = Duration::from_secs(60);

/// A one-shot answer to a sudo prompt in an interactive terminal.
///
/// The command is run with a custom `sudo -p` prompt containing `%u`, and only the
/// *expanded* form (`… password for alice: `) is matched. The shell's echo of the
/// command line carries the literal `%u`, so typing the command can never match
/// its own prompt. A wrong password makes sudo ask again — by then this has
/// disarmed, and the user answers it themselves instead of the app retrying a bad
/// password until the account locks.
pub struct SudoAutofill {
    prompt: String,
    password: String,
    deadline: Instant,
    // The end of the output seen so far: a prompt can arrive split across two
    // channel messages.
    tail: String,
}

impl SudoAutofill {
    pub fn new(prompt: String, password: String) -> Self {
        Self { prompt, password, deadline: Instant::now() + SUDO_AUTOFILL_WINDOW, tail: String::new() }
    }

    pub fn expired(&self) -> bool {
        Instant::now() > self.deadline
    }

    /// Feeds terminal output; true once the prompt has appeared.
    pub fn feed(&mut self, text: &str) -> bool {
        self.tail.push_str(text);
        if self.tail.contains(&self.prompt) {
            return true;
        }
        // Keep just enough to catch a prompt split at any point.
        let keep = self.prompt.len();
        if self.tail.len() > keep {
            let mut cut = self.tail.len() - keep;
            while !self.tail.is_char_boundary(cut) {
                cut += 1;
            }
            self.tail.drain(..cut);
        }
        false
    }

    /// The bytes to type: sudo reads a line, and a PTY's Enter is `\r`.
    pub fn answer(self) -> Vec<u8> {
        format!("{}\r", self.password).into_bytes()
    }
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
        let mut sudo_autofill: Option<SudoAutofill> = None;

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
                            Some(SessionCommand::WriteWithSudo { line, autofill }) => {
                                sudo_autofill = Some(autofill);
                                if channel.data_bytes(line).await.is_err() {
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
                                if sudo_autofill.as_ref().is_some_and(SudoAutofill::expired) {
                                    sudo_autofill = None;
                                }
                                if sudo_autofill.as_mut().is_some_and(|a| a.feed(&text)) {
                                    let answer = sudo_autofill.take().unwrap().answer();
                                    if channel.data_bytes(answer).await.is_err() {
                                        break;
                                    }
                                }
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
            // A new shell will never show the prompt this was waiting for.
            sudo_autofill = None;

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
                        state.monitor.lock().unwrap().remove(&session_id);
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
                        state.monitor.lock().unwrap().remove(&session_id);
                    }
                    break 'connection;
                }
            }
        }

        let _ = session.disconnect(russh::Disconnect::ByApplication, "", "en").await;
    });

    Ok((tx, session))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROMPT: &str = "[sudo via SshManager] password for alice: ";

    #[test]
    fn ignores_the_echoed_command_and_matches_the_expanded_prompt() {
        let mut autofill = SudoAutofill::new(PROMPT.into(), "pw".into());
        // The shell's echo of the typed line has the unexpanded `%u`.
        assert!(!autofill.feed("$ sudo -p '[sudo via SshManager] password for %u: ' sh -c 'apt update'\r
"));
        assert!(autofill.feed(PROMPT));
        assert_eq!(autofill.answer(), b"pw\r");
    }

    #[test]
    fn matches_a_prompt_split_across_messages() {
        let mut autofill = SudoAutofill::new(PROMPT.into(), "pw".into());
        assert!(!autofill.feed("noise ".repeat(100).as_str()));
        assert!(!autofill.feed("[sudo via SshMan"));
        assert!(autofill.feed("ager] password for alice: "));
    }

    #[test]
    fn a_different_user_is_not_answered() {
        let mut autofill = SudoAutofill::new(PROMPT.into(), "pw".into());
        assert!(!autofill.feed("[sudo via SshManager] password for bob: "));
        // Multi-byte output does not trip the tail trimming.
        assert!(!autofill.feed(&"ā".repeat(200)));
    }
}

