use std::time::Duration;

use serde::Deserialize;
use tauri::State;
use uuid::Uuid;

use crate::ssh::pty::{SessionCommand, SudoAutofill};
use crate::ssh::updates::{self, UpdateStatus};
use crate::state::AppState;

use super::sftp::{session_ssh, sudo_password};

// `exec::run` has no deadline of its own (see tasks/lessons.md). `apt list` reads
// every package list and can take several seconds on a slow VPS, so this is more
// generous than the monitor's sample timeout.
const STATUS_TIMEOUT: Duration = Duration::from_secs(30);

// The prompt sudo is told to print. `%u` is expanded by sudo, so only the real
// prompt — never the shell's echo of the command line — reads "password for
// <name>". It also tells the user why the password is being typed for them.
const PROMPT_PREFIX: &str = "[sudo via SshManager] password for ";

/// What to type. An enum rather than a string from the frontend, so nothing
/// free-form is ever run under the saved sudo password.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HostAction {
    Upgrade,
    Reboot,
}

impl HostAction {
    // Free of backslashes, `!` and newlines: typed into whatever the login shell
    // is, fish and csh included (see tasks/lessons.md). The upgrade is one `sh -c`
    // so a single sudo covers both steps and there is one prompt to answer.
    fn command(self) -> &'static str {
        match self {
            Self::Upgrade => "sh -c 'apt update && apt upgrade'",
            Self::Reboot => "reboot",
        }
    }
}

/// Package-update status. Unprivileged, read-only, and never refreshes the
/// package lists itself — pending counts are as of the host's last `apt update`.
#[tauri::command]
pub async fn updates_status(state: State<'_, AppState>, session_id: String) -> Result<UpdateStatus, String> {
    let (ssh, _) = session_ssh(&state, &session_id)?;
    tokio::time::timeout(STATUS_TIMEOUT, updates::collect(&ssh))
        .await
        .map_err(|_| "the host did not answer the update check in time".to_string())?
        .map_err(|e| e.to_string())
}

/// Types the upgrade (or the reboot) into the pane's own terminal and presses
/// Enter, so apt's raw output and dpkg's questions stay in front of the user.
///
/// When the connection has a saved login password, sudo's prompt is answered with
/// it — once, and only on seeing the exact prompt (see `SudoAutofill`). The
/// password goes straight from Credential Manager into the SSH channel; it never
/// reaches the webview. Without one (key auth), sudo prompts as usual.
#[tauri::command]
pub fn updates_run(
    state: State<'_, AppState>,
    session_id: String,
    as_root: bool,
    action: HostAction,
) -> Result<(), String> {
    let (_, connection_id) = session_ssh(&state, &session_id)?;

    let command = if as_root {
        SessionCommand::Write(format!("{}\r", action.command()).into_bytes())
    } else {
        let line = format!("sudo -p '{PROMPT_PREFIX}%u: ' {}\r", action.command()).into_bytes();
        let username = Uuid::parse_str(&connection_id)
            .ok()
            .and_then(|id| state.connections.lock().unwrap().get(&id))
            .map(|profile| profile.username);
        match (username, sudo_password(&state, &connection_id)) {
            (Some(username), Some(password)) => SessionCommand::WriteWithSudo {
                line,
                autofill: SudoAutofill::new(format!("{PROMPT_PREFIX}{username}: "), password),
            },
            _ => SessionCommand::Write(line),
        }
    };

    let sessions = state.sessions.lock().unwrap();
    let session = sessions.get(&session_id).ok_or_else(|| "session not found".to_string())?;
    session.cmd_tx.send(command).map_err(|_| "session already closed".to_string())
}
