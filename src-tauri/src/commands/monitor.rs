use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Deserialize;
use tauri::State;

use crate::ssh::client::Client;
use crate::ssh::monitor::{self, ListeningSocket, RawSample, Snapshot};
use crate::ssh::{self};
use crate::state::{AppState, MonitorState};

use super::sftp::{last_error_line, run_with_sudo, session_ssh};

// `exec::run` waits on the channel until it ends, with no deadline of its own, so
// a `df` blocked on a dead NFS mount would leave the poll pending forever and the
// panel would simply stop updating with nothing on screen to say why.
const SAMPLE_TIMEOUT: Duration = Duration::from_secs(20);

// Two polls landing together (a manual refresh on top of the timer) share one
// answer rather than each paying a round trip.
const CACHE_WINDOW: Duration = Duration::from_millis(500);

// Past this, the stored counters are useless as a baseline: the panel was paused,
// or sat in a hidden tab, and subtracting across the whole gap would report a
// smooth average of the last few minutes as though it were "now".
const STALE_BASELINE: Duration = Duration::from_secs(15);

// Gap between the two collections taken when the stored baseline was too old to
// use. Long enough for a busy process to accumulate tens of clock ticks, short
// enough not to be felt as a stall.
const PRIME_GAP: Duration = Duration::from_millis(300);

// Every collection goes through here, so the timeout cannot be forgotten on one of
// the paths.
async fn collect(ssh: &Arc<russh::client::Handle<Client>>) -> Result<RawSample, String> {
    tokio::time::timeout(SAMPLE_TIMEOUT, monitor::collect(ssh))
        .await
        .map_err(|_| "the host did not answer in time — a hung network mount will do this".to_string())?
        .map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KillSignal {
    Term,
    Kill,
    Int,
    Hup,
}

impl KillSignal {
    // An enum rather than a string so nothing free-form ever reaches a command
    // line, however the frontend is changed later.
    fn name(self) -> &'static str {
        match self {
            Self::Term => "TERM",
            Self::Kill => "KILL",
            Self::Int => "INT",
            Self::Hup => "HUP",
        }
    }
}

fn monitor_slot(state: &AppState, session_id: &str) -> Arc<tokio::sync::Mutex<MonitorState>> {
    // The std mutex is released by the end of this function, before any await --
    // the same shape `get_sftp` uses to hand an Arc out of a blocking lock.
    let mut sessions = state.monitor.lock().unwrap();
    sessions.entry(session_id.to_string()).or_default().clone()
}

#[tauri::command]
pub async fn monitor_sample(state: State<'_, AppState>, session_id: String) -> Result<Snapshot, String> {
    let (ssh, _) = session_ssh(&state, &session_id)?;
    let slot = monitor_slot(&state, &session_id);

    // Held across the collection, so overlapping polls queue instead of both
    // diffing against each other's sample and halving the measured interval.
    let mut monitor = slot.lock().await;

    if let Some((taken, snapshot)) = &monitor.recent {
        if taken.elapsed() < CACHE_WINDOW {
            return Ok(snapshot.clone());
        }
    }

    // What the new sample gets subtracted from:
    //
    // - a fresh stored sample: the ordinary case, no extra cost.
    // - a *stale* one: the panel was paused, or sat in a hidden tab. Subtracting
    //   across that gap would report a smooth average of the last few minutes as
    //   though it were "now", so it is thrown away and replaced by a primer taken
    //   a moment ago. That costs one extra round trip, but it means a single
    //   refresh after a pause still shows real numbers instead of dashes the user
    //   has to refresh again to clear.
    // - nothing at all: the panel just opened. Here the dash is the right answer —
    //   priming would double the time to first paint on every open, and the next
    //   tick is a couple of seconds away.
    let baseline = match monitor.previous.take() {
        Some((taken, sample)) if taken.elapsed() < STALE_BASELINE => Some(sample),
        Some(_) => {
            let primer = collect(&ssh).await?;
            tokio::time::sleep(PRIME_GAP).await;
            Some(primer)
        }
        None => None,
    };

    let sample = collect(&ssh).await?;
    let snapshot = monitor::diff(baseline.as_ref(), &sample);

    monitor.previous = Some((Instant::now(), sample));
    monitor.recent = Some((Instant::now(), snapshot.clone()));
    Ok(snapshot)
}

/// Signals one process. `start_ticks` is the `starttime` the panel displayed, and
/// it must still match: pids are recycled, so between rendering a row and clicking
/// it the number can belong to something else entirely, and killing *that* is
/// exactly the accident this command exists to prevent.
#[tauri::command]
pub async fn monitor_kill(
    state: State<'_, AppState>,
    session_id: String,
    pid: u32,
    start_ticks: u64,
    signal: KillSignal,
) -> Result<(), String> {
    if pid <= 1 {
        return Err("pid 1 is the init system — signalling it would take the host down".into());
    }
    let (ssh, _) = session_ssh(&state, &session_id)?;

    // `pid` is a u32, so it renders as digits and nothing here needs quoting.
    let stat = ssh::exec::run(&ssh, &format!("cat /proc/{pid}/stat"), None)
        .await
        .map_err(|e| e.to_string())?;
    let live = monitor::starttime_of(&stat.stdout)
        .ok_or_else(|| format!("process {pid} is no longer running"))?;
    if live != start_ticks {
        return Err(format!(
            "pid {pid} now belongs to a different process than the one listed — refresh and try again"
        ));
    }

    let command = format!("kill -{} {}", signal.name(), pid);
    let attempt = ssh::exec::run(&ssh, &command, None).await.map_err(|e| e.to_string())?;
    if attempt.status == 0 {
        return Ok(());
    }
    let refusal = last_error_line(&attempt.stderr).unwrap_or("the host refused the signal").to_string();

    // Someone else's process: the same escalation the SFTP write path uses, which
    // already declines to offer a key passphrase to sudo.
    run_with_sudo(&state, &session_id, &command)
        .await
        .map_err(|reason| format!("could not signal {pid} ({refusal}), and sudo could not either: {reason}"))
}

/// Listening TCP/UDP sockets. Naming the process behind one needs root, so this
/// runs unprivileged and reports the names it can see — escalating every couple of
/// seconds for a column that is nice to have would be the wrong trade.
#[tauri::command]
pub async fn monitor_ports(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<ListeningSocket>, String> {
    let (ssh, _) = session_ssh(&state, &session_id)?;

    // Probe-and-fall-back on stdout rather than exit status, the shape
    // `sftp_dir_sizes` uses for `du -b` vs `du -k`: `ss` exits nonzero on a host
    // where it cannot name every process, yet still prints every socket.
    let ss = ssh::exec::run(&ssh, "ss -H -tulpn", None).await.map_err(|e| e.to_string())?;
    if !ss.stdout.trim().is_empty() {
        return Ok(monitor::parse_ss(&ss.stdout));
    }

    let netstat = ssh::exec::run(&ssh, "netstat -tulpn", None).await.map_err(|e| e.to_string())?;
    if !netstat.stdout.trim().is_empty() {
        return Ok(monitor::parse_netstat(&netstat.stdout));
    }

    Err(last_error_line(&netstat.stderr)
        .or_else(|| last_error_line(&ss.stderr))
        .unwrap_or("neither ss nor netstat is available on this host")
        .to_string())
}
