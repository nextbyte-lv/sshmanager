# SshManager package-update status collector (Debian/Ubuntu).
#
# One shot per session, not part of the monitor's 2-second sample: none of this
# changes faster than an apt run, and `apt list` costs a second or two.
#
# Sent to a bare `sh` on the exec channel's STDIN for the same reason monitor.sh
# is — see the header there. Same contract too: independent sections, a final
# `@@end` sentinel, and no `set -e`. Everything here is readable without root, so
# the status check never asks for sudo.
export LC_ALL=C

echo "@@os"
cat /etc/os-release 2>/dev/null

echo "@@apt"
command -v apt-get >/dev/null 2>&1 && echo present

# The remote clock and uptime: every age below is computed against these, never
# against the local PC, whose clock may disagree with the server's.
echo "@@clock"
echo "now $(date +%s)"
echo "uptime $(cut -d' ' -f1 /proc/uptime 2>/dev/null)"
echo "uid $(id -u)"

# Last successful `apt update`. No single file records it on every system:
# update-success-stamp is only written by apt.systemd.daily (or Ubuntu's
# update-notifier hook), while a manual `apt update` that fetched anything
# leaves its mark on the lists directory. The newest of these is the answer.
echo "@@refreshed"
for path in /var/lib/apt/periodic/update-success-stamp /var/lib/apt/lists /var/lib/apt/lists/partial; do
  [ -e "$path" ] && stat -c '%Y' "$path" 2>/dev/null
done

# dpkg rewrites its status database on every install, upgrade and removal, and
# the file is world-readable — unlike /var/log/apt/history.log on most hosts.
echo "@@changed"
stat -c '%Y' /var/lib/dpkg/status 2>/dev/null

# Judged against the package lists as they were at the last refresh — this does
# not fetch anything, so it is only as fresh as `@@refreshed`.
echo "@@pending"
apt list --upgradable 2>/dev/null

echo "@@reboot"
[ -f /var/run/reboot-required ] && echo required
cat /var/run/reboot-required.pkgs 2>/dev/null

# Plain Debian only creates reboot-required when update-notifier-common or
# needrestart is installed, so also compare the running kernel against the most
# recently installed one. Empty /boot (containers, WSL) leaves `newest` unset.
echo "@@kernel"
echo "running $(uname -r)"
newest=$(ls -t /boot/vmlinuz-* 2>/dev/null | head -n 1)
[ -n "$newest" ] && echo "newest ${newest#/boot/vmlinuz-}"

# Whether unattended-upgrades is installed and switched on, and the daily
# apt-daily-upgrade job that runs it.
#
# Two sources, because neither is enough alone:
# - the *service* knows how its last run ended, but only since boot — systemd
#   keeps that in memory. The monotonic timestamp (microseconds since boot) is
#   used because its format is the same on every systemd version; zero means it
#   has not run since the last boot.
# - the *timer* remembers its last trigger across reboots (Persistent=true keeps
#   a stamp file), and knows the next one. `list-timers --output=json` gives both
#   as epoch microseconds; `systemctl show`'s timestamps are human-formatted
#   whatever `--timestamp=` says. Older systemd has no JSON output, so the stamp
#   file's mtime is the fallback for the last trigger.
echo "@@auto"
[ -x /usr/bin/unattended-upgrade ] && echo installed
apt-config dump 2>/dev/null | grep -E '^APT::Periodic::(Unattended-Upgrade|Update-Package-Lists) '
if command -v systemctl >/dev/null 2>&1; then
  systemctl show apt-daily-upgrade.service -p LoadState -p ExecMainExitTimestampMonotonic -p Result 2>/dev/null
  echo "timers $(systemctl list-timers apt-daily-upgrade.timer --all --output=json 2>/dev/null)"
fi
stat -c 'stamp %Y' /var/lib/systemd/timers/stamp-apt-daily-upgrade.timer 2>/dev/null

echo "@@end"
