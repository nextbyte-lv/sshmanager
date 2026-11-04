//! Package-update status of a Debian/Ubuntu host: when it was last refreshed and
//! upgraded, what is pending, and whether it is waiting for a reboot.
//!
//! Read-only and unprivileged. Running the upgrade itself is not done here — the
//! frontend types it into the pane's own terminal, so the user sees apt's raw
//! output, answers sudo and dpkg's config-file prompts themselves, and notices
//! if the connection drops mid-upgrade.

use std::sync::Arc;

use serde::Serialize;

use super::client::Client;
use super::monitor::split_sections;
use super::{exec, SshError};

// `.replace("\r\n", "\n")` at the call site, for the same reason monitor.sh needs
// it: a CRLF checkout would ship `\r` to a remote POSIX shell.
const COLLECT_SCRIPT: &str = include_str!("updates.sh");

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PendingPackage {
    pub name: String,
    pub version: String,
    pub from_version: Option<String>,
    /// Offered by a `-security` suite (`stable-security`, `noble-security`…).
    pub security: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct UpdateStatus {
    /// False on anything that is not apt-based; the frontend then shows nothing
    /// rather than a status it cannot back up.
    pub supported: bool,
    pub distro: Option<String>,
    /// The login user is root, so the upgrade command must not start with
    /// `sudo` — minimal Debian installs logged into as root often lack it.
    pub is_root: bool,
    /// The host's own clock (epoch seconds). Every age is measured against this
    /// rather than the local PC's clock.
    pub checked_at: i64,
    /// Last successful `apt update`, as best the filesystem records it.
    pub lists_refreshed_at: Option<i64>,
    /// Last install, upgrade or removal (dpkg status database mtime).
    pub packages_changed_at: Option<i64>,
    pub pending: Vec<PendingPackage>,
    pub pending_security: usize,
    pub reboot_required: bool,
    /// Packages that asked for the reboot, when the host records them.
    pub reboot_packages: Vec<String>,
    pub running_kernel: Option<String>,
    /// Most recently installed kernel, when it differs from the running one.
    pub newer_kernel: Option<String>,
    pub unattended_installed: bool,
    /// `APT::Periodic::Unattended-Upgrade` is set to a non-zero value.
    pub unattended_enabled: bool,
    /// When the daily `apt-daily-upgrade` job last ran. Known across reboots (from
    /// the timer), unlike its result.
    pub unattended_last_run: Option<i64>,
    /// systemd's `Result` for that run: `success`, `exit-code`, `timeout`… Only
    /// known when the run happened since the last boot.
    pub unattended_last_result: Option<String>,
    /// When the timer will next fire, where systemd can say (JSON output, 250+).
    pub unattended_next_run: Option<i64>,
}

pub async fn collect(ssh: &Arc<russh::client::Handle<Client>>) -> Result<UpdateStatus, SshError> {
    let script = COLLECT_SCRIPT.replace("\r\n", "\n");
    let output = exec::run(ssh, "sh", Some(&script)).await?;
    parse(&output.stdout).map_err(SshError::Monitor)
}

pub fn parse(stdout: &str) -> Result<UpdateStatus, String> {
    let sections = split_sections(stdout);
    if !sections.contains_key("end") {
        return Err(if sections.is_empty() {
            "the host produced no output; its login shell may run a forced command".into()
        } else {
            "the update check was cut short before it finished".into()
        });
    }
    let section = |name: &str| sections.get(name).copied().unwrap_or("");

    let os = parse_os_release(section("os"));
    let os_field = |key: &str| os.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
    let debian_like = [os_field("ID"), os_field("ID_LIKE")]
        .into_iter()
        .flatten()
        .any(|ids| ids.split_whitespace().any(|id| id == "debian" || id == "ubuntu"));
    let apt_present = section("apt").trim() == "present";

    let mut status = UpdateStatus {
        supported: debian_like && apt_present,
        distro: os_field("PRETTY_NAME").map(str::to_string),
        ..Default::default()
    };

    let mut uptime = None;
    for line in section("clock").lines() {
        match line.split_once(' ') {
            Some(("now", v)) => status.checked_at = v.trim().parse().unwrap_or(0),
            Some(("uptime", v)) => uptime = v.trim().parse::<f64>().ok(),
            Some(("uid", v)) => status.is_root = v.trim() == "0",
            _ => {}
        }
    }

    status.lists_refreshed_at = section("refreshed").lines().filter_map(|l| l.trim().parse().ok()).max();
    status.packages_changed_at = section("changed").lines().find_map(|l| l.trim().parse().ok());

    status.pending = section("pending").lines().filter_map(parse_upgradable).collect();
    status.pending_security = status.pending.iter().filter(|p| p.security).count();

    for line in section("reboot").lines().map(str::trim).filter(|l| !l.is_empty()) {
        if line == "required" {
            status.reboot_required = true;
        } else if !status.reboot_packages.iter().any(|p| p == line) {
            status.reboot_packages.push(line.to_string());
        }
    }

    let mut newest_kernel = None;
    for line in section("kernel").lines() {
        match line.split_once(' ') {
            Some(("running", v)) if !v.trim().is_empty() => status.running_kernel = Some(v.trim().to_string()),
            Some(("newest", v)) if !v.trim().is_empty() => newest_kernel = Some(v.trim().to_string()),
            _ => {}
        }
    }
    if let (Some(running), Some(newest)) = (&status.running_kernel, newest_kernel) {
        if *running != newest {
            status.newer_kernel = Some(newest);
            status.reboot_required = true;
        }
    }

    parse_auto(section("auto"), uptime, &mut status);
    Ok(status)
}

fn parse_os_release(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let (key, value) = line.trim().split_once('=')?;
            let value = value.trim().trim_matches('"').trim_matches('\'');
            Some((key.to_string(), value.to_string()))
        })
        .collect()
}

/// One line of `apt list --upgradable`:
/// `libssl3t64/stable-security 3.5.6-1~deb13u1 amd64 [upgradable from: 3.5.1-1]`.
/// The `Listing...` header and anything else without a `name/suite` first word
/// are skipped.
fn parse_upgradable(line: &str) -> Option<PendingPackage> {
    let mut words = line.split_whitespace();
    let (name, suites) = words.next()?.split_once('/')?;
    let version = words.next()?;
    let from_version = line
        .split_once("[upgradable from: ")
        .and_then(|(_, rest)| rest.split_once(']'))
        .map(|(from, _)| from.trim().to_string());
    Some(PendingPackage {
        name: name.to_string(),
        version: version.to_string(),
        from_version,
        // Ubuntu lists every suite offering it, comma-separated:
        // `noble-updates,noble-security`.
        security: suites.split(',').any(|suite| suite.ends_with("-security")),
    })
}

fn parse_auto(text: &str, uptime: Option<f64>, status: &mut UpdateStatus) {
    let mut loaded = false;
    let mut exit_monotonic_us: u64 = 0;
    let mut result = None;
    let mut timer_last = None;
    let mut stamp = None;

    for line in text.lines().map(str::trim) {
        if line == "installed" {
            status.unattended_installed = true;
        } else if let Some(value) = line.strip_prefix("APT::Periodic::Unattended-Upgrade ") {
            // `"1";` — a count of days, where zero (or an empty string) means off.
            let days = value.trim_end_matches(';').trim_matches('"');
            status.unattended_enabled = days.parse::<u32>().map_or(false, |d| d > 0);
        } else if let Some(value) = line.strip_prefix("LoadState=") {
            loaded = value == "loaded";
        } else if let Some(value) = line.strip_prefix("ExecMainExitTimestampMonotonic=") {
            exit_monotonic_us = value.parse().unwrap_or(0);
        } else if let Some(value) = line.strip_prefix("Result=") {
            result = Some(value.to_string());
        } else if let Some(json) = line.strip_prefix("timers ") {
            let (last, next) = parse_timer_json(json);
            timer_last = last;
            status.unattended_next_run = next;
        } else if let Some(value) = line.strip_prefix("stamp ") {
            stamp = value.parse::<i64>().ok().filter(|&t| t > 0);
        }
    }

    // `systemctl show` answers for a unit that does not exist, with
    // `Result=success` and a zero timestamp — hence LoadState. Zero on a loaded
    // unit means it has not run since boot.
    let since_boot = match uptime {
        Some(uptime) if loaded && exit_monotonic_us > 0 => {
            let boot_time = status.checked_at as f64 - uptime;
            Some((boot_time + exit_monotonic_us as f64 / 1_000_000.0).round() as i64)
        }
        _ => None,
    };

    if since_boot.is_some() {
        status.unattended_last_run = since_boot;
        status.unattended_last_result = result;
    } else {
        // Rebooted since the last run: the timer still knows when, but how it
        // ended went with the old boot. No result rather than a guessed one.
        status.unattended_last_run = timer_last.or(stamp);
    }
}

/// `systemctl list-timers --output=json`: `[{"next":µs,"last":µs,…}]`. Zero, or
/// a missing field, means never / not scheduled.
fn parse_timer_json(json: &str) -> (Option<i64>, Option<i64>) {
    let Ok(timers) = serde_json::from_str::<Vec<serde_json::Value>>(json) else {
        return (None, None);
    };
    let Some(timer) = timers.first() else {
        return (None, None);
    };
    let seconds = |key: &str| {
        timer.get(key).and_then(serde_json::Value::as_u64).filter(|&us| us > 0).map(|us| (us / 1_000_000) as i64)
    };
    (seconds("last"), seconds("next"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEBIAN13: &str = include_str!("testdata/updates-debian13.txt");

    #[test]
    fn real_debian_sample() {
        let status = parse(DEBIAN13).unwrap();
        assert!(status.supported);
        assert_eq!(status.distro.as_deref(), Some("Debian GNU/Linux 13 (trixie)"));
        assert!(!status.is_root);
        assert_eq!(status.lists_refreshed_at, Some(1780838903));
        assert_eq!(status.packages_changed_at, Some(1780839149));
        assert_eq!(status.pending.len(), 39);
        assert_eq!(status.pending_security, 4);
        assert_eq!(
            status.pending.iter().find(|p| p.name == "libkrb5-3"),
            Some(&PendingPackage {
                name: "libkrb5-3".into(),
                version: "1.21.3-5+deb13u1".into(),
                from_version: Some("1.21.3-5".into()),
                security: true,
            })
        );
        // WSL: no /boot kernels to compare against, and no reboot-required file.
        assert!(!status.reboot_required);
        assert_eq!(status.newer_kernel, None);
        assert!(!status.unattended_installed);
        assert!(!status.unattended_enabled);
        // apt-daily-upgrade.service finished 302.7s after a boot 22288.19s before
        // the sample was taken.
        assert_eq!(status.checked_at, 1793816190);
        assert_eq!(status.unattended_last_run, Some((1793816190.0f64 - 22288.19 + 302.7388).round() as i64));
        assert_eq!(status.unattended_last_result.as_deref(), Some("success"));
    }

    #[test]
    fn ubuntu_with_reboot_and_unattended() {
        let out = "@@os\nID=ubuntu\nID_LIKE=debian\nPRETTY_NAME=\"Ubuntu 24.04.1 LTS\"\n\
                   @@apt\npresent\n@@clock\nnow 1000000\nuptime 500.0\nuid 0\n\
                   @@refreshed\n999000\n@@changed\n998000\n\
                   @@pending\nListing...\nopenssl/noble-updates,noble-security 3.0.13-0ubuntu3.4 amd64 [upgradable from: 3.0.13-0ubuntu3.1]\n\
                   @@reboot\nrequired\nlinux-image-6.8.0-45-generic\nlinux-image-6.8.0-45-generic\n\
                   @@kernel\nrunning 6.8.0-41-generic\nnewest 6.8.0-45-generic\n\
                   @@auto\ninstalled\nAPT::Periodic::Update-Package-Lists \"1\";\nAPT::Periodic::Unattended-Upgrade \"1\";\n\
                   LoadState=loaded\nExecMainExitTimestampMonotonic=100000000\nResult=exit-code\n@@end\n";
        let status = parse(out).unwrap();
        assert!(status.supported);
        assert!(status.is_root);
        assert_eq!(status.pending_security, 1);
        assert!(status.reboot_required);
        assert_eq!(status.reboot_packages, vec!["linux-image-6.8.0-45-generic"]);
        assert_eq!(status.newer_kernel.as_deref(), Some("6.8.0-45-generic"));
        assert!(status.unattended_installed && status.unattended_enabled);
        assert_eq!(status.unattended_last_run, Some(1000000 - 500 + 100));
        assert_eq!(status.unattended_last_result.as_deref(), Some("exit-code"));
    }

    #[test]
    fn missing_unit_and_disabled_unattended() {
        let out = "@@os\nID=debian\n@@apt\npresent\n@@clock\nnow 10\nuptime 5\nuid 1000\n\
                   @@auto\ninstalled\nAPT::Periodic::Unattended-Upgrade \"0\";\n\
                   LoadState=not-found\nExecMainExitTimestampMonotonic=0\nResult=success\n@@end\n";
        let status = parse(out).unwrap();
        assert!(status.unattended_installed);
        assert!(!status.unattended_enabled);
        assert_eq!(status.unattended_last_run, None);
        assert_eq!(status.unattended_last_result, None);
        assert_eq!(status.lists_refreshed_at, None);
        assert!(status.pending.is_empty());
    }

    #[test]
    fn non_apt_host_is_unsupported() {
        let out = "@@os\nID=fedora\nPRETTY_NAME=\"Fedora Linux 41\"\n@@apt\n@@clock\nnow 1\n@@end\n";
        assert!(!parse(out).unwrap().supported);
        // An apt binary alone, on something that is not Debian-like, is not enough.
        let out = "@@os\nID=alpine\n@@apt\npresent\n@@end\n";
        assert!(!parse(out).unwrap().supported);
    }

    // The home-server case: the job ran this morning, then the host rebooted, so
    // the service has no run since boot but the timer still remembers it.
    #[test]
    fn rebooted_since_the_last_run_still_shows_when() {
        let out = "@@os\nID=debian\n@@apt\npresent\n@@clock\nnow 1793819089\nuptime 600\nuid 1000\n\
                   @@auto\ninstalled\nAPT::Periodic::Unattended-Upgrade \"1\";\n\
                   Result=success\nExecMainExitTimestampMonotonic=0\nLoadState=loaded\n\
                   timers [{\"next\":1793851434516332,\"left\":1793851434516332,\"last\":1793788011031843,\
                   \"passed\":302460200,\"unit\":\"apt-daily-upgrade.timer\",\"activates\":\"apt-daily-upgrade.service\"}]\n\
                   stamp 1793788011\n@@end\n";
        let status = parse(out).unwrap();
        assert_eq!(status.unattended_last_run, Some(1793788011));
        // How it ended went with the old boot; `Result=success` here is the
        // default for a unit that has not run, not a real result.
        assert_eq!(status.unattended_last_result, None);
        assert_eq!(status.unattended_next_run, Some(1793851434));
    }

    #[test]
    fn old_systemd_without_json_falls_back_to_the_stamp_file() {
        let out = "@@os\nID=ubuntu\n@@apt\npresent\n@@clock\nnow 2000\nuptime 100\nuid 1000\n\
                   @@auto\nLoadState=loaded\nExecMainExitTimestampMonotonic=0\nResult=success\n\
                   timers \nstamp 1500\n@@end\n";
        let status = parse(out).unwrap();
        assert_eq!(status.unattended_last_run, Some(1500));
        assert_eq!(status.unattended_next_run, None);
    }

    #[test]
    fn truncated_output_is_an_error() {
        assert!(parse("@@os\nID=debian\n").is_err());
        assert!(parse("").is_err());
    }
}
