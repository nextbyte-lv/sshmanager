// Remote host monitoring: one exec round trip per poll, deltas computed here.
//
// Everything that matters in a task manager -- CPU%, network and disk throughput
// -- is a *counter delta*, unreadable from a single sample. (`ps pcpu` is a
// process-lifetime average, not current load.) So `collect` takes a raw sample
// and `diff` turns two consecutive ones into the `Snapshot` the panel renders.
//
// The parsers here carry the whole risk of the feature: nearly every way to get
// /proc wrong yields a plausible number rather than an error, which is why they
// are pure functions with tests rather than something eyeballed in the UI.

use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;

use super::client::Client;
use super::{exec, SshError};

const COLLECT_SCRIPT: &str = include_str!("monitor.sh");

// A sector in /proc/diskstats is always 512 bytes, whatever the device's real
// logical or physical block size says.
const SECTOR_BYTES: u64 = 512;

// Filesystem types that are never a disk anyone wants a usage bar for.
//
// `squashfs` earns its place because a stock Ubuntu has dozens of snap mounts, all
// 100% full by construction. `tmpfs` is RAM rather than a disk -- and there is a
// lot of it: a WSL Debian mounts twenty, which would push the real volumes off the
// card entirely. What it holds is already on the memory gauge.
const PSEUDO_FS: &[&str] = &[
    "autofs", "binfmt_misc", "bpf", "cgroup", "cgroup2", "configfs", "debugfs", "devpts",
    "devtmpfs", "efivarfs", "fusectl", "hugetlbfs", "mqueue", "nsfs", "overlay", "proc",
    "pstore", "ramfs", "rootfs", "securityfs", "selinuxfs", "squashfs", "sysfs", "tmpfs",
    "tracefs",
];

// ---------------------------------------------------------------- raw sample

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CpuTimes {
    pub user: u64,
    pub nice: u64,
    pub system: u64,
    pub idle: u64,
    pub iowait: u64,
    pub irq: u64,
    pub softirq: u64,
    pub steal: u64,
    pub guest: u64,
    pub guest_nice: u64,
}

impl CpuTimes {
    // htop's decomposition. `user` in /proc/stat already includes `guest`, and
    // `nice` already includes `guest_nice`, so summing all ten fields raw inflates
    // the denominator on any host running KVM guests.
    fn total(&self) -> u64 {
        let usertime = self.user.saturating_sub(self.guest);
        let nicetime = self.nice.saturating_sub(self.guest_nice);
        usertime
            + nicetime
            + self.system
            + self.irq
            + self.softirq
            + self.idle
            + self.iowait
            + self.steal
            + self.guest
            + self.guest_nice
    }

    // Time nothing was waiting on the CPU. iowait counts as idle here, which is
    // what people mean by "busy"; it is also reported separately.
    fn idle_all(&self) -> u64 {
        self.idle + self.iowait
    }

    // `None` on any counter going backwards -- a reboot, a 32-bit wrap, or our own
    // reconnect. Discarding beats clamping: a clamp reports a saturated link as
    // 0 B/s, which reads as "nothing is happening".
    fn delta(&self, earlier: &Self) -> Option<Self> {
        Some(Self {
            user: self.user.checked_sub(earlier.user)?,
            nice: self.nice.checked_sub(earlier.nice)?,
            system: self.system.checked_sub(earlier.system)?,
            idle: self.idle.checked_sub(earlier.idle)?,
            iowait: self.iowait.checked_sub(earlier.iowait)?,
            irq: self.irq.checked_sub(earlier.irq)?,
            softirq: self.softirq.checked_sub(earlier.softirq)?,
            steal: self.steal.checked_sub(earlier.steal)?,
            guest: self.guest.checked_sub(earlier.guest)?,
            guest_nice: self.guest_nice.checked_sub(earlier.guest_nice)?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct RawProc {
    pub pid: u32,
    pub state: String,
    pub ppid: u32,
    pub utime: u64,
    pub stime: u64,
    pub threads: u32,
    pub starttime: u64,
    pub rss_pages: u64,
    pub comm: String,
}

impl RawProc {
    fn cpu_ticks(&self) -> u64 {
        self.utime + self.stime
    }
}

#[derive(Debug, Clone)]
pub struct RawFs {
    pub device: String,
    pub total_kb: u64,
    pub used_kb: u64,
    pub avail_kb: u64,
    pub mount: String,
}

#[derive(Debug, Clone, Default)]
pub struct RawSample {
    pub os: String,
    pub kernel: String,
    pub arch: String,
    pub clk_tck: u64,
    pub pagesize: u64,
    pub remote_now: i64,
    pub cpu_total: CpuTimes,
    pub cpus: Vec<(String, CpuTimes)>,
    pub btime: Option<i64>,
    pub uptime: f64,
    /// `(interface, rx_bytes, tx_bytes)`
    pub net: Vec<(String, u64, u64)>,
    /// `(device, sectors_read, sectors_written)`
    pub disks: Vec<(String, u64, u64)>,
    pub procs: Vec<RawProc>,
    /// pid -> `(user, full command line)`
    pub ps: HashMap<u32, (String, String)>,
    /// Raw /proc/meminfo, values in kB.
    pub mem: HashMap<String, u64>,
    pub load: [f64; 3],
    pub cpu_model: Option<String>,
    pub pressure: Vec<(String, f32)>,
    pub filesystems: Vec<RawFs>,
    /// `(mount point, fs type, options)`
    pub mounts: Vec<(String, String, String)>,
    pub cgroup: HashMap<String, String>,
    pub block_devices: Vec<String>,
}

// ------------------------------------------------------------------ snapshot

#[derive(Debug, Clone, Serialize)]
pub struct HostInfo {
    pub os: String,
    pub kernel: String,
    pub arch: String,
    pub cpu_model: String,
    pub cores: usize,
    pub boot_time: Option<i64>,
    pub uptime_seconds: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CpuUsage {
    pub busy: f32,
    pub user: f32,
    pub system: f32,
    pub iowait: f32,
    pub steal: f32,
    pub per_core: Vec<f32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryUsage {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub buffers_bytes: u64,
    pub cache_bytes: u64,
    /// True when `MemAvailable` was missing and `used` had to be approximated.
    pub estimated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SwapUsage {
    pub total_bytes: u64,
    pub used_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Filesystem {
    pub device: String,
    pub mount: String,
    pub fs_type: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    /// `used / (used + available)`, which is what `df` itself prints. Dividing by
    /// the total instead disagrees with `df -h` on every ext4 volume, because the
    /// root-reserved 5% is in neither `used` nor `available`.
    pub used_percent: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct NetInterface {
    pub name: String,
    pub rx_bytes_per_sec: f64,
    pub tx_bytes_per_sec: f64,
    pub rx_total: u64,
    pub tx_total: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiskIo {
    pub device: String,
    pub read_bytes_per_sec: f64,
    pub write_bytes_per_sec: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Process {
    pub pid: u32,
    pub ppid: u32,
    pub user: String,
    pub name: String,
    pub command: String,
    pub state: String,
    pub threads: u32,
    /// Per-core scale, matching htop and `top`: a process saturating four of eight
    /// cores reads 400. `None` until there is a previous sample to diff against.
    pub cpu_percent: Option<f32>,
    pub memory_bytes: u64,
    pub memory_percent: f32,
    pub started_at: Option<i64>,
    /// `starttime` in clock ticks. Carried so a kill can prove the pid still refers
    /// to the process the user clicked on.
    pub start_ticks: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub host: HostInfo,
    pub cpu: Option<CpuUsage>,
    pub memory: MemoryUsage,
    pub swap: Option<SwapUsage>,
    pub load: [f64; 3],
    pub pressure: Vec<(String, f32)>,
    pub filesystems: Vec<Filesystem>,
    pub network: Vec<NetInterface>,
    pub disks: Vec<DiskIo>,
    pub processes: Vec<Process>,
    /// Reasons a number on screen may not mean what it appears to.
    pub warnings: Vec<String>,
    /// First sample of a session: rates are not knowable yet.
    pub measuring: bool,
    pub sampled_at: i64,
    pub process_count: usize,
}

// ---------------------------------------------------------------- collection

pub async fn collect(ssh: &Arc<russh::client::Handle<Client>>) -> Result<RawSample, SshError> {
    // The script goes on stdin to a bare `sh`, never as `sh -c '<script>'`: the
    // login shell may be fish or csh, both of which reinterpret characters inside
    // single quotes that the awk program depends on. See monitor.sh's header.
    //
    // CRLF would arrive as a trailing \r on every line and break the remote shell.
    // `.gitattributes` pins the file to LF; this normalises anyway, because a fresh
    // clone on a machine with core.autocrlf=true is otherwise a silent, baffling
    // failure a long way from its cause.
    let script = COLLECT_SCRIPT.replace("\r\n", "\n");
    let output = exec::run(ssh, "sh", Some(&script)).await?;

    // Deliberately not checking the exit status: `cat /proc/[0-9]*/stat` exits
    // nonzero whenever any process ended mid-read, which is most polls on a busy
    // host. The `@@end` sentinel is the real completeness check -- its absence
    // means the channel died, or a ForceCommand/nologin shell ran something else
    // entirely. Same reasoning as `sftp_dir_sizes` keying on stdout, not status.
    parse(&output.stdout).map_err(SshError::Monitor)
}

fn parse(stdout: &str) -> Result<RawSample, String> {
    let sections = split_sections(stdout);
    if !sections.contains_key("end") {
        return Err(if sections.is_empty() {
            "the host produced no monitor output; its login shell may run a forced command".into()
        } else {
            "the monitor sample was cut short before it finished".into()
        });
    }

    let section = |name: &str| sections.get(name).copied().unwrap_or("");

    let uname: Vec<&str> = section("uname").split_whitespace().collect();
    let os = uname.first().copied().unwrap_or_default().to_string();
    if !os.eq_ignore_ascii_case("Linux") {
        return Err(format!(
            "the process monitor reads Linux /proc, and this host reports {}",
            if os.is_empty() { "an unknown system" } else { os.as_str() }
        ));
    }

    let conf = parse_key_values(section("conf"));
    let (cpu_total, cpus, btime) = parse_stat(section("stat"));

    Ok(RawSample {
        os,
        kernel: uname.get(1).copied().unwrap_or_default().to_string(),
        arch: uname.get(2).copied().unwrap_or_default().to_string(),
        // USER_HZ is 100 on every mainstream architecture regardless of CONFIG_HZ.
        clk_tck: conf.get("clk_tck").and_then(|v| v.parse().ok()).unwrap_or(100),
        // 65536 on 64K-page arm64 and ppc64le, so hardcoding 4096 would be a
        // silent 16x error on RSS for those hosts.
        pagesize: conf.get("pagesize").and_then(|v| v.parse().ok()).unwrap_or(4096),
        remote_now: conf.get("now").and_then(|v| v.parse().ok()).unwrap_or(0),
        cpu_total,
        cpus,
        btime,
        uptime: section("uptime")
            .split_whitespace()
            .next()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0),
        net: parse_netdev(section("netdev")),
        disks: parse_diskstats(section("diskstats")),
        procs: parse_procs(section("procs")),
        ps: parse_ps(section("ps")),
        mem: parse_meminfo(section("meminfo")),
        load: parse_loadavg(section("loadavg")),
        cpu_model: section("cpuinfo")
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(str::to_string),
        pressure: parse_pressure(section("pressure")),
        filesystems: parse_df(section("df")),
        mounts: parse_mounts(section("mounts")),
        cgroup: parse_key_values(section("cgroup")),
        block_devices: section("blockdevs").split_whitespace().map(str::to_string).collect(),
    })
}

fn split_sections(stdout: &str) -> HashMap<&str, &str> {
    let mut sections = HashMap::new();
    let mut name: Option<&str> = None;
    let mut start = 0usize;
    let mut cursor = 0usize;

    for line in stdout.split_inclusive('\n') {
        if let Some(header) = line.trim_end().strip_prefix("@@") {
            if let Some(previous) = name {
                sections.insert(previous, &stdout[start..cursor]);
            }
            name = Some(header);
            start = cursor + line.len();
        }
        cursor += line.len();
    }
    if let Some(previous) = name {
        sections.insert(previous, &stdout[start..cursor]);
    }
    sections
}

fn parse_key_values(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|line| {
            let (key, value) = line.trim().split_once(' ')?;
            Some((key.to_string(), value.trim().to_string()))
        })
        .collect()
}

fn parse_stat(text: &str) -> (CpuTimes, Vec<(String, CpuTimes)>, Option<i64>) {
    let mut total = CpuTimes::default();
    // Keyed by label, never by position: /proc/stat lists only *online* CPUs, so a
    // core going offline would otherwise shift every later core's history by one.
    let mut cores = Vec::new();
    let mut btime = None;

    for line in text.lines() {
        let mut fields = line.split_whitespace();
        let Some(label) = fields.next() else { continue };
        if label == "btime" {
            btime = fields.next().and_then(|v| v.parse().ok());
            continue;
        }
        if !label.starts_with("cpu") {
            continue;
        }
        let v: Vec<u64> = fields.map(|f| f.parse().unwrap_or(0)).collect();
        let at = |i: usize| v.get(i).copied().unwrap_or(0);
        let times = CpuTimes {
            user: at(0),
            nice: at(1),
            system: at(2),
            idle: at(3),
            iowait: at(4),
            irq: at(5),
            softirq: at(6),
            steal: at(7),
            guest: at(8),
            guest_nice: at(9),
        };
        if label == "cpu" {
            total = times;
        } else {
            cores.push((label.to_string(), times));
        }
    }
    (total, cores, btime)
}

fn parse_meminfo(text: &str) -> HashMap<String, u64> {
    text.lines()
        .filter_map(|line| {
            let (key, rest) = line.split_once(':')?;
            let value = rest.split_whitespace().next()?.parse().ok()?;
            Some((key.to_string(), value))
        })
        .collect()
}

fn parse_loadavg(text: &str) -> [f64; 3] {
    let mut load = [0.0; 3];
    for (slot, field) in load.iter_mut().zip(text.split_whitespace()) {
        *slot = field.parse().unwrap_or(0.0);
    }
    load
}

// Split on the first ':' rather than on whitespace: a counter wide enough to touch
// the colon (`eth0:1234567890123`) leaves no space between them, and whitespace
// splitting then shifts every field by one, so rates read as zero or as garbage.
fn parse_netdev(text: &str) -> Vec<(String, u64, u64)> {
    text.lines()
        .filter_map(|line| {
            let (name, rest) = line.split_once(':')?;
            let v: Vec<u64> = rest.split_whitespace().map(|f| f.parse().unwrap_or(0)).collect();
            Some((name.trim().to_string(), *v.first()?, *v.get(8)?))
        })
        .collect()
}

fn parse_diskstats(text: &str) -> Vec<(String, u64, u64)> {
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            Some((f.get(2)?.to_string(), f.get(5)?.parse().ok()?, f.get(9)?.parse().ok()?))
        })
        .collect()
}

fn parse_procs(text: &str) -> Vec<RawProc> {
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 9 {
                return None;
            }
            Some(RawProc {
                pid: f[0].parse().ok()?,
                state: f[1].to_string(),
                ppid: f[2].parse().unwrap_or(0),
                utime: f[3].parse().unwrap_or(0),
                stime: f[4].parse().unwrap_or(0),
                threads: f[5].parse().unwrap_or(1),
                starttime: f[6].parse().unwrap_or(0),
                rss_pages: f[7].parse().unwrap_or(0),
                // The collector substitutes '_' for whitespace in comm so the row
                // stays one token per field; a name that genuinely contained '_'
                // is indistinguishable, which costs nothing worth fixing.
                comm: f[8].to_string(),
            })
        })
        .collect()
}

// `ps` prints argv raw, and argv may contain a newline -- which would otherwise
// shift every following row, silently pairing one process with another's command
// line. A line is only a row if it starts with a pid.
fn parse_ps(text: &str) -> HashMap<u32, (String, String)> {
    let mut rows: HashMap<u32, (String, String)> = HashMap::new();
    let mut last: Option<u32> = None;

    for line in text.lines() {
        // Columns are padded to a fixed width, so the gap between two of them is
        // any run of spaces -- splitting on single whitespace characters would
        // hand back empty fields.
        let trimmed = line.trim_start();
        let head = trimmed.split_whitespace().next().unwrap_or("");
        let is_row = !head.is_empty() && head.bytes().all(|b| b.is_ascii_digit());

        if !is_row {
            // A continuation of the previous row's command line.
            if let Some(row) = last.and_then(|pid| rows.get_mut(&pid)) {
                row.1.push(' ');
                row.1.push_str(line.trim());
            }
            continue;
        }

        let Ok(pid) = head.parse::<u32>() else { continue };
        let after_pid = trimmed[head.len()..].trim_start();
        let user = after_pid.split_whitespace().next().unwrap_or("").to_string();
        let args = after_pid[user.len()..].trim().to_string();
        rows.insert(pid, (user, args));
        last = Some(pid);
    }
    rows
}

fn parse_pressure(text: &str) -> Vec<(String, f32)> {
    text.lines()
        .filter_map(|line| {
            let (resource, value) = line.trim().split_once(' ')?;
            let avg = value.split_once('=')?.1.parse().ok()?;
            Some((resource.to_string(), avg))
        })
        .collect()
}

// `df -P` guarantees one line per filesystem, so the only ambiguity left is a
// mount point containing spaces -- which is why the tail is rejoined, not indexed.
fn parse_df(text: &str) -> Vec<RawFs> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 6 {
                return None;
            }
            Some(RawFs {
                device: f[0].to_string(),
                total_kb: f[1].parse().ok()?,
                used_kb: f[2].parse().ok()?,
                avail_kb: f[3].parse().ok()?,
                mount: f[5..].join(" "),
            })
        })
        .collect()
}

fn parse_mounts(text: &str) -> Vec<(String, String, String)> {
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            Some((
                f.first()?.to_string(),
                f.get(1)?.to_string(),
                f.get(2).copied().unwrap_or("").to_string(),
            ))
        })
        .collect()
}

// --------------------------------------------------------------------- diff

/// Turns the newest sample into what the panel renders. `previous` is `None` only
/// for the first sample of a session, or after one was discarded because a counter
/// went backwards; every rate is then `None` and the panel says it is measuring.
pub fn diff(previous: Option<&RawSample>, current: &RawSample) -> Snapshot {
    // Elapsed time comes from the *remote* uptime counter, not from our poll
    // interval: SSH latency and scheduler jitter make the local interval wrong by
    // tens of percent, which would silently scale every byte-per-second figure.
    let elapsed = previous
        .map(|prev| current.uptime - prev.uptime)
        .filter(|seconds| *seconds > 0.0);

    let cpu = previous.and_then(|prev| cpu_usage(prev, current));
    let measuring = cpu.is_none();

    let mut warnings = Vec::new();
    let memory = memory_usage(current, &mut warnings);
    collect_warnings(current, &mut warnings);

    Snapshot {
        host: host_info(current),
        cpu,
        memory,
        swap: swap_usage(current),
        load: current.load,
        pressure: current.pressure.clone(),
        filesystems: filesystems(current),
        network: network(previous, current, elapsed),
        disks: disk_io(previous, current, elapsed),
        processes: processes(previous, current, memory_total_bytes(current)),
        warnings,
        measuring,
        sampled_at: current.remote_now,
        process_count: current.procs.len(),
    }
}

fn host_info(sample: &RawSample) -> HostInfo {
    HostInfo {
        os: sample.os.clone(),
        kernel: sample.kernel.clone(),
        arch: sample.arch.clone(),
        cpu_model: sample
            .cpu_model
            .clone()
            .filter(|model| !model.is_empty())
            .unwrap_or_else(|| sample.arch.clone()),
        cores: sample.cpus.len(),
        boot_time: sample.btime,
        uptime_seconds: sample.uptime,
    }
}

fn cpu_usage(previous: &RawSample, current: &RawSample) -> Option<CpuUsage> {
    let delta = current.cpu_total.delta(&previous.cpu_total)?;
    let total = delta.total();
    if total == 0 {
        return None;
    }
    let percent = |value: u64| (value as f64 / total as f64 * 100.0) as f32;

    // Cores are matched by label rather than by index, so a core going offline
    // between samples drops out instead of shifting everything after it.
    let earlier: HashMap<&str, &CpuTimes> =
        previous.cpus.iter().map(|(label, times)| (label.as_str(), times)).collect();
    let per_core = current
        .cpus
        .iter()
        .map(|(label, times)| {
            earlier
                .get(label.as_str())
                .and_then(|was| times.delta(was))
                .filter(|d| d.total() > 0)
                .map(|d| ((d.total() - d.idle_all()) as f64 / d.total() as f64 * 100.0) as f32)
                .unwrap_or(0.0)
        })
        .collect();

    Some(CpuUsage {
        busy: percent(total - delta.idle_all()),
        user: percent(
            delta.user.saturating_sub(delta.guest) + delta.nice.saturating_sub(delta.guest_nice),
        ),
        system: percent(delta.system + delta.irq + delta.softirq),
        iowait: percent(delta.iowait),
        // On a throttled VPS this single number is the whole diagnosis, and it
        // costs nothing to surface.
        steal: percent(delta.steal),
        per_core,
    })
}

fn memory_total_bytes(sample: &RawSample) -> u64 {
    kb(sample.mem.get("MemTotal"))
}

fn kb(value: Option<&u64>) -> u64 {
    // /proc/meminfo's "kB" is really KiB.
    value.copied().unwrap_or(0) * 1024
}

fn memory_usage(sample: &RawSample, warnings: &mut Vec<String>) -> MemoryUsage {
    let total = kb(sample.mem.get("MemTotal"));
    let buffers = kb(sample.mem.get("Buffers"));
    let shmem = kb(sample.mem.get("Shmem"));
    let cache = kb(sample.mem.get("Cached")) + kb(sample.mem.get("SReclaimable"));
    let cache = cache.saturating_sub(shmem);

    // `MemTotal - MemAvailable` is htop 3's and `free --available`'s definition,
    // and the one that actually predicts an OOM. It is deliberately larger than
    // `free`'s classic "used", which assumes all page cache is reclaimable.
    let (available, estimated) = match sample.mem.get("MemAvailable") {
        Some(value) => (value * 1024, false),
        // Absent before kernel 3.14. Defaulting it to zero would report every such
        // host as 100% full, so approximate and say so.
        None => {
            warnings.push(
                "this kernel does not report MemAvailable, so memory used is an estimate".into(),
            );
            (kb(sample.mem.get("MemFree")) + buffers + cache, true)
        }
    };

    MemoryUsage {
        total_bytes: total,
        used_bytes: total.saturating_sub(available),
        available_bytes: available,
        buffers_bytes: buffers,
        cache_bytes: cache,
        estimated,
    }
}

fn swap_usage(sample: &RawSample) -> Option<SwapUsage> {
    let total = kb(sample.mem.get("SwapTotal"));
    if total == 0 {
        return None;
    }
    let free = kb(sample.mem.get("SwapFree"));
    let cached = kb(sample.mem.get("SwapCached"));
    Some(SwapUsage { total_bytes: total, used_bytes: total.saturating_sub(free + cached) })
}

fn filesystems(sample: &RawSample) -> Vec<Filesystem> {
    let types: HashMap<&str, &str> =
        sample.mounts.iter().map(|(mount, fs, _)| (mount.as_str(), fs.as_str())).collect();

    let mut seen = Vec::new();
    let mut out = Vec::new();
    for fs in &sample.filesystems {
        let fs_type = types.get(fs.mount.as_str()).copied().unwrap_or("");
        if fs.total_kb == 0 || PSEUDO_FS.contains(&fs_type) {
            continue;
        }
        // A bind mount reports the same device and the same numbers under a second
        // path; one row per underlying volume is what a usage list is for.
        let identity = (fs.device.clone(), fs.total_kb, fs.used_kb);
        if seen.contains(&identity) {
            continue;
        }
        seen.push(identity);

        let used = fs.used_kb * 1024;
        let available = fs.avail_kb * 1024;
        let denominator = used + available;
        out.push(Filesystem {
            device: fs.device.clone(),
            mount: fs.mount.clone(),
            fs_type: fs_type.to_string(),
            total_bytes: fs.total_kb * 1024,
            used_bytes: used,
            available_bytes: available,
            used_percent: if denominator == 0 {
                0.0
            } else {
                (used as f64 / denominator as f64 * 100.0) as f32
            },
        });
    }
    out
}

fn network(
    previous: Option<&RawSample>,
    current: &RawSample,
    elapsed: Option<f64>,
) -> Vec<NetInterface> {
    let earlier: HashMap<&str, (u64, u64)> = previous
        .map(|prev| prev.net.iter().map(|(n, rx, tx)| (n.as_str(), (*rx, *tx))).collect())
        .unwrap_or_default();

    current
        .net
        .iter()
        .filter(|(name, _, _)| name != "lo")
        .map(|(name, rx, tx)| {
            let rate = |now: u64, before: Option<u64>| {
                match (before, elapsed) {
                    // checked_sub, not a clamp: an interface bounced or a 32-bit
                    // counter wrapped, and "0 B/s" on a busy link is a lie.
                    (Some(was), Some(seconds)) => {
                        now.checked_sub(was).map(|d| d as f64 / seconds).unwrap_or(0.0)
                    }
                    _ => 0.0,
                }
            };
            let before = earlier.get(name.as_str());
            NetInterface {
                name: name.clone(),
                rx_bytes_per_sec: rate(*rx, before.map(|b| b.0)),
                tx_bytes_per_sec: rate(*tx, before.map(|b| b.1)),
                rx_total: *rx,
                tx_total: *tx,
            }
        })
        .collect()
}

fn disk_io(previous: Option<&RawSample>, current: &RawSample, elapsed: Option<f64>) -> Vec<DiskIo> {
    let earlier: HashMap<&str, (u64, u64)> = previous
        .map(|prev| prev.disks.iter().map(|(n, r, w)| (n.as_str(), (*r, *w))).collect())
        .unwrap_or_default();

    current
        .disks
        .iter()
        // /proc/diskstats lists sda, sda1 and dm-0 alike, so a "total" over it
        // double- or triple-counts. /sys/block is exactly the set of whole devices;
        // per-device rows and no total is the honest presentation.
        .filter(|(name, _, _)| current.block_devices.iter().any(|dev| dev == name))
        .filter(|(name, _, _)| !name.starts_with("loop") && !name.starts_with("ram"))
        .map(|(name, reads, writes)| {
            let rate = |now: u64, before: Option<u64>| match (before, elapsed) {
                (Some(was), Some(seconds)) => now
                    .checked_sub(was)
                    .map(|d| (d * SECTOR_BYTES) as f64 / seconds)
                    .unwrap_or(0.0),
                _ => 0.0,
            };
            let before = earlier.get(name.as_str());
            DiskIo {
                device: name.clone(),
                read_bytes_per_sec: rate(*reads, before.map(|b| b.0)),
                write_bytes_per_sec: rate(*writes, before.map(|b| b.1)),
            }
        })
        .collect()
}

fn processes(previous: Option<&RawSample>, current: &RawSample, total_memory: u64) -> Vec<Process> {
    // Keyed by (pid, starttime), never by pid alone: Linux reuses pids, and diffing
    // a fresh process against a dead one's counters yields either a negative delta
    // or an absurd spike. htop keys the same way for the same reason.
    let earlier: HashMap<(u32, u64), u64> = previous
        .map(|prev| prev.procs.iter().map(|p| ((p.pid, p.starttime), p.cpu_ticks())).collect())
        .unwrap_or_default();

    // The aggregate `cpu` line's delta is already cores x elapsed ticks, so using
    // it as the denominator makes this independent of the kernel's HZ -- the ticks
    // cancel. Multiplying back up by the core count gives htop's per-core scale.
    let cpu_denominator = previous
        .and_then(|prev| current.cpu_total.delta(&prev.cpu_total))
        .map(|delta| delta.total())
        .filter(|total| *total > 0);
    let cores = current.cpus.len().max(1) as f64;

    current
        .procs
        .iter()
        .map(|proc| {
            let ps = current.ps.get(&proc.pid);
            let cpu_percent = cpu_denominator.and_then(|denominator| {
                let was = earlier.get(&(proc.pid, proc.starttime))?;
                let ticks = proc.cpu_ticks().checked_sub(*was)?;
                Some((ticks as f64 / denominator as f64 * 100.0 * cores) as f32)
            });
            let memory_bytes = proc.rss_pages * current.pagesize;

            Process {
                pid: proc.pid,
                ppid: proc.ppid,
                user: ps.map(|(user, _)| user.clone()).unwrap_or_default(),
                name: proc.comm.clone(),
                // Falls back to comm rather than dropping the row: busybox `ps`
                // rejects the field syntax the collector asks for.
                command: ps
                    .map(|(_, args)| args.clone())
                    .filter(|args| !args.is_empty())
                    .unwrap_or_else(|| proc.comm.clone()),
                state: proc.state.clone(),
                threads: proc.threads,
                cpu_percent,
                memory_bytes,
                memory_percent: if total_memory == 0 {
                    0.0
                } else {
                    (memory_bytes as f64 / total_memory as f64 * 100.0) as f32
                },
                // Against the remote clock, via boot time -- never `local_now -
                // uptime`, which breaks on clock skew and on host suspend.
                started_at: current
                    .btime
                    .filter(|_| current.clk_tck > 0)
                    .map(|btime| btime + (proc.starttime / current.clk_tck) as i64),
                start_ticks: proc.starttime,
            }
        })
        .collect()
}

// The cases where a perfectly plausible number is a wrong number. Showing one of
// these unlabelled is worse than showing nothing.
fn collect_warnings(sample: &RawSample, warnings: &mut Vec<String>) {
    if let Some(options) = sample
        .mounts
        .iter()
        .find(|(mount, fs, _)| mount == "/proc" || fs == "proc")
        .map(|(_, _, options)| options)
    {
        if options.contains("hidepid=2") || options.contains("hidepid=invisible") {
            warnings.push(
                "this host hides other users' processes (hidepid), so the list is only yours".into(),
            );
        }
    }

    if sample.ps.is_empty() && !sample.procs.is_empty() {
        warnings
            .push("`ps` is unavailable here, so command lines fall back to process names".into());
    }

    // Inside a cgroup-limited container every /proc figure is the *host's*: a
    // container capped at 512 MB on a 256 GB box reports 256 GB, and every number
    // looks fine while meaning nothing.
    let host_memory = memory_total_bytes(sample);
    let limit = sample
        .cgroup
        .get("mem_max")
        .or_else(|| sample.cgroup.get("mem_limit_v1"))
        .and_then(|value| value.trim().parse::<u64>().ok());
    if let Some(limit) = limit {
        if limit > 0 && host_memory > 0 && limit < host_memory {
            warnings.push(format!(
                "this looks like a container limited to {} MiB; the figures below are the host's",
                limit / 1024 / 1024
            ));
        }
    }
}

// ------------------------------------------------------- kill safety + ports

/// `starttime` (field 22) out of a raw `/proc/<pid>/stat` line, splitting on the
/// last `)` for the same reason the collector's awk does.
///
/// This is what lets a kill prove the pid still refers to the process the user
/// clicked: pids get recycled, and between rendering a row and clicking it the
/// number can belong to something else entirely.
pub fn starttime_of(stat_line: &str) -> Option<u64> {
    let tail = stat_line.rfind(')')?;
    stat_line.get(tail + 1..)?.split_whitespace().nth(19)?.parse().ok()
}

#[derive(Debug, Clone, Serialize)]
pub struct ListeningSocket {
    pub protocol: String,
    pub address: String,
    pub port: String,
    /// `None` when the listener belongs to another user -- naming it needs root,
    /// and the panel says so rather than escalating uninvited.
    pub process: Option<String>,
}

// An address is split at its *last* colon, so `[::]:22` and `0.0.0.0:22` both
// come apart correctly.
fn split_address(endpoint: &str) -> (String, String) {
    match endpoint.rfind(':') {
        Some(at) => (endpoint[..at].to_string(), endpoint[at + 1..].to_string()),
        None => (endpoint.to_string(), String::new()),
    }
}

/// `ss -H -tulpn`: netid, state, recv-q, send-q, local, peer, then an optional
/// `users:(("name",pid=N,fd=M))`.
pub fn parse_ss(text: &str) -> Vec<ListeningSocket> {
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            let (address, port) = split_address(f.get(4)?);
            let process = f.get(6).and_then(|users| {
                let name = users.split('"').nth(1)?;
                match users.split("pid=").nth(1).and_then(|rest| rest.split(',').next()) {
                    Some(pid) => Some(format!("{name} ({pid})")),
                    None => Some(name.to_string()),
                }
            });
            Some(ListeningSocket { protocol: f.first()?.to_string(), address, port, process })
        })
        .collect()
}

/// `netstat -tulpn`: proto, recv-q, send-q, local, foreign, then `LISTEN` for tcp
/// only, then `pid/program` (or `-` without the privilege to see it).
pub fn parse_netstat(text: &str) -> Vec<ListeningSocket> {
    text.lines()
        .filter(|line| line.starts_with("tcp") || line.starts_with("udp"))
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            let (address, port) = split_address(f.get(3)?);
            let process = f.last().filter(|last| last.contains('/')).and_then(|last| {
                let (pid, name) = last.split_once('/')?;
                Some(format!("{name} ({pid})"))
            });
            Some(ListeningSocket { protocol: f.first()?.to_string(), address, port, process })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // One stat line per real /proc/pid/stat shape, including the trap: comm holds
    // both a space (substituted by the collector) and a closing paren.
    const PROCS: &str = "1 S 0 100 50 1 500 1000 systemd\n\
                         42 R 1 200 100 4 900 2000 my_)_app\n";

    fn core_times(stat_user: u64) -> CpuTimes {
        CpuTimes { user: stat_user / 2, idle: 500 + stat_user / 2, ..Default::default() }
    }

    fn sample(stat_user: u64, uptime: f64, proc_ticks: u64) -> RawSample {
        RawSample {
            os: "Linux".into(),
            clk_tck: 100,
            pagesize: 4096,
            uptime,
            btime: Some(1_700_000_000),
            // Idle advances in step with busy time, so `stat_user` ticks of work
            // between two samples means `2 * stat_user` aggregate ticks passed --
            // i.e. the machine was half busy, across two cores.
            cpu_total: CpuTimes { user: stat_user, idle: 1000 + stat_user, ..Default::default() },
            cpus: vec![
                ("cpu0".into(), core_times(stat_user)),
                ("cpu1".into(), core_times(stat_user)),
            ],
            procs: {
                let mut procs = parse_procs(PROCS);
                procs[1].utime = proc_ticks;
                procs
            },
            mem: [("MemTotal".to_string(), 1024 * 1024), ("MemAvailable".to_string(), 512 * 1024)]
                .into_iter()
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn splits_sections_and_requires_the_sentinel() {
        let complete = "@@uname\nLinux 6.1.0 x86_64\n@@loadavg\n0.5 0.4 0.3\n@@end\n";
        let sections = split_sections(complete);
        assert_eq!(sections.get("uname").copied(), Some("Linux 6.1.0 x86_64\n"));
        assert_eq!(sections.get("loadavg").copied(), Some("0.5 0.4 0.3\n"));
        assert!(sections.contains_key("end"));

        // A sample cut short mid-flight must be refused, not half-parsed: the exit
        // status cannot be trusted, so the sentinel is the only completeness proof.
        assert!(parse("@@uname\nLinux 6.1.0 x86_64\n@@stat\ncpu 1 0 0 1\n").is_err());
        assert!(parse("").is_err());
    }

    #[test]
    fn refuses_a_non_linux_host_rather_than_guessing() {
        let error = parse("@@uname\nDarwin 23.0.0 arm64\n@@end\n").unwrap_err();
        assert!(error.contains("Darwin"), "{error}");
    }

    #[test]
    fn projected_process_rows_survive_a_comm_containing_a_paren() {
        let procs = parse_procs(PROCS);
        assert_eq!(procs.len(), 2);
        assert_eq!(procs[1].pid, 42);
        assert_eq!(procs[1].ppid, 1);
        assert_eq!(procs[1].utime, 200);
        assert_eq!(procs[1].stime, 100);
        assert_eq!(procs[1].threads, 4);
        assert_eq!(procs[1].starttime, 900);
        assert_eq!(procs[1].rss_pages, 2000);
        assert_eq!(procs[1].comm, "my_)_app");
    }

    #[test]
    fn net_counters_parse_when_no_space_follows_the_colon() {
        // A counter wide enough to touch the colon is the classic silent shift.
        let text = "Inter-|   Receive\n face |bytes\n\
                    eth0:1234567890123 1 2 3 4 5 6 7 999 8\n  lo: 10 1 2 3 4 5 6 7 20 8\n";
        let net = parse_netdev(text);
        assert_eq!(net.iter().find(|(n, _, _)| n == "eth0").map(|(_, rx, tx)| (*rx, *tx)), Some((1234567890123, 999)));
        assert_eq!(net.iter().find(|(n, _, _)| n == "lo").map(|(_, rx, tx)| (*rx, *tx)), Some((10, 20)));
    }

    #[test]
    fn ps_rows_absorb_a_newline_inside_a_command_line() {
        let text = "1 root /sbin/init\n42 arturs.vitolins /usr/bin/thing --flag=line one\ncontinued here\n7 www-data nginx\n";
        let rows = parse_ps(text);
        // The stray line joins the row it belongs to instead of shifting the rest.
        assert_eq!(rows[&42].1, "/usr/bin/thing --flag=line one continued here");
        // A username longer than ps's default 8-character field stays intact.
        assert_eq!(rows[&42].0, "arturs.vitolins");
        assert_eq!(rows[&7].1, "nginx");
    }

    #[test]
    fn df_percentage_matches_df_not_size_division() {
        // 5% root-reserved: 100 total, 40 used, 55 available. df prints 42%, and
        // dividing by the total would print 40%.
        let text = "Filesystem 1024-blocks Used Available Capacity Mounted on\n\
                    /dev/sda1 100 40 55 42% /mnt/my disk\n";
        let parsed = parse_df(text);
        assert_eq!(parsed[0].mount, "/mnt/my disk");

        let mut sample = sample(0, 0.0, 0);
        sample.filesystems = parsed;
        sample.mounts = vec![("/mnt/my disk".into(), "ext4".into(), "rw".into())];
        let out = filesystems(&sample);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].used_percent.round(), 42.0);
    }

    #[test]
    fn pseudo_filesystems_and_bind_mounts_are_dropped() {
        let mut sample = sample(0, 0.0, 0);
        sample.filesystems = vec![
            RawFs { device: "/dev/sda1".into(), total_kb: 100, used_kb: 40, avail_kb: 55, mount: "/".into() },
            RawFs { device: "/dev/sda1".into(), total_kb: 100, used_kb: 40, avail_kb: 55, mount: "/bind".into() },
            RawFs { device: "/dev/loop0".into(), total_kb: 50, used_kb: 50, avail_kb: 0, mount: "/snap/core".into() },
        ];
        sample.mounts = vec![
            ("/".into(), "ext4".into(), "rw".into()),
            ("/bind".into(), "ext4".into(), "rw".into()),
            ("/snap/core".into(), "squashfs".into(), "ro".into()),
        ];
        let out = filesystems(&sample);
        assert_eq!(out.iter().map(|fs| fs.mount.as_str()).collect::<Vec<_>>(), vec!["/"]);
    }

    #[test]
    fn cpu_total_does_not_double_count_guest_time() {
        // On a KVM host `user` already contains `guest`; summing all ten fields raw
        // would inflate the denominator and understate every percentage.
        let times = CpuTimes { user: 100, nice: 20, guest: 40, guest_nice: 5, idle: 100, ..Default::default() };
        assert_eq!(times.total(), (100 - 40) + (20 - 5) + 100 + 40 + 5);
    }

    #[test]
    fn cpu_and_process_percentages_are_measured_against_the_aggregate() {
        // 200 aggregate ticks pass, 100 of them busy -> 50% across the machine.
        let first = sample(0, 100.0, 0);
        let second = sample(100, 102.0, 100);

        let snapshot = diff(Some(&first), &second);
        let cpu = snapshot.cpu.expect("a second sample yields real percentages");
        assert_eq!(cpu.busy.round(), 50.0);
        assert_eq!(cpu.per_core.len(), 2);
        assert!(!snapshot.measuring);

        // The process burned 100 of those 200 ticks. Per machine that is 50%; on
        // htop's per-core scale with two cores it reads 100.
        let busy = snapshot.processes.iter().find(|p| p.pid == 42).unwrap();
        assert_eq!(busy.cpu_percent.map(f32::round), Some(100.0));
        // RSS is in pages, so 2000 pages at 4 KiB is just under 8 MiB.
        assert_eq!(busy.memory_bytes, 2000 * 4096);
        // starttime 900 ticks at 100 Hz is 9 seconds after boot.
        assert_eq!(busy.started_at, Some(1_700_000_009));
    }

    #[test]
    fn the_first_sample_reports_no_rates_rather_than_zeroes() {
        let snapshot = diff(None, &sample(100, 100.0, 100));
        assert!(snapshot.measuring);
        assert!(snapshot.cpu.is_none());
        assert!(snapshot.processes.iter().all(|p| p.cpu_percent.is_none()));
    }

    #[test]
    fn a_counter_going_backwards_discards_the_rate_instead_of_reporting_zero() {
        // What a reboot, a 32-bit wrap, or our own reconnect looks like.
        let before = sample(500, 900.0, 500);
        let after = sample(10, 20.0, 10);
        let snapshot = diff(Some(&before), &after);
        assert!(snapshot.cpu.is_none(), "a negative delta must not read as 0% busy");
        assert!(snapshot.measuring);
    }

    #[test]
    fn pid_reuse_does_not_produce_a_spike() {
        let before = sample(0, 100.0, 5_000);
        let mut after = sample(100, 102.0, 10);
        // Same pid, different starttime: a new process wearing a recycled number.
        after.procs[1].starttime = 999_999;
        let snapshot = diff(Some(&before), &after);
        let recycled = snapshot.processes.iter().find(|p| p.pid == 42).unwrap();
        assert!(recycled.cpu_percent.is_none(), "a reused pid must not diff against the dead one");
    }

    #[test]
    fn a_kernel_without_mem_available_says_so_instead_of_reporting_a_full_host() {
        let mut sample = sample(0, 0.0, 0);
        sample.mem = [
            ("MemTotal".to_string(), 1000),
            ("MemFree".to_string(), 400),
            ("Cached".to_string(), 200),
        ]
        .into_iter()
        .collect();

        let snapshot = diff(None, &sample);
        assert!(snapshot.memory.estimated);
        assert_eq!(snapshot.memory.available_bytes, 600 * 1024);
        assert!(snapshot.warnings.iter().any(|w| w.contains("MemAvailable")), "{:?}", snapshot.warnings);
    }

    #[test]
    fn a_cgroup_limit_below_the_host_total_is_flagged() {
        let mut sample = sample(0, 0.0, 0);
        // MemTotal in the fixture is 1 GiB; the cgroup allows 64 MiB.
        sample.cgroup = [("mem_max".to_string(), (64 * 1024 * 1024).to_string())].into_iter().collect();
        let snapshot = diff(None, &sample);
        assert!(snapshot.warnings.iter().any(|w| w.contains("container")), "{:?}", snapshot.warnings);
    }

    #[test]
    fn hidepid_is_reported_from_the_mount_options_already_collected() {
        let mut sample = sample(0, 0.0, 0);
        sample.mounts = vec![("/proc".into(), "proc".into(), "rw,hidepid=2".into())];
        let snapshot = diff(None, &sample);
        assert!(snapshot.warnings.iter().any(|w| w.contains("hidepid")), "{:?}", snapshot.warnings);
    }

    // Two consecutive real samples, collected by monitor.sh itself against a live
    // Linux kernel with `yes > /dev/null` running throughout, then trimmed to
    // fifteen processes. The synthetic tests above prove the arithmetic; this one
    // proves the arithmetic is being fed what the kernel actually prints -- which
    // is where every silent wrong number in this file would come from.
    #[test]
    fn a_real_pair_of_samples_reproduces_what_the_host_was_doing() {
        let first = parse(include_str!("testdata/sample1.txt")).expect("sample 1 parses");
        let second = parse(include_str!("testdata/sample2.txt")).expect("sample 2 parses");

        assert_eq!(first.clk_tck, 100);
        assert_eq!(first.pagesize, 4096);
        assert!(first.cpu_model.as_deref().unwrap_or("").contains("Intel"), "{:?}", first.cpu_model);

        let snapshot = diff(Some(&first), &second);
        assert!(!snapshot.measuring);
        assert_eq!(snapshot.host.cores, 24);

        // One core of twenty-four pinned, so the machine reads a little over 1/24.
        let cpu = snapshot.cpu.expect("two samples give a real percentage");
        assert_eq!(cpu.per_core.len(), 24);
        assert!((4.0..12.0).contains(&cpu.busy), "machine busy was {}", cpu.busy);

        // `yes` burned 201 ticks across the 2.02s between samples. On htop's
        // per-core scale that is one saturated core, and it must land on 100 --
        // not 4 (divided by cores) and not 10000 (ticks mistaken for percent).
        let busy = snapshot.processes.iter().find(|p| p.name == "yes").expect("the load process");
        let percent = busy.cpu_percent.expect("a diffed process has a percentage");
        assert!((95.0..105.0).contains(&percent), "yes read {percent}%");
        // 424 pages of RSS, in pages rather than kB.
        assert_eq!(busy.memory_bytes, 424 * 4096);
        // The `ps` join supplied the owner and the command line.
        assert!(!busy.user.is_empty());
        assert_eq!(busy.command, "yes");
        assert!(busy.started_at.is_some());

        // The host mounts twenty tmpfs filesystems and a pile of loop devices;
        // none of them belongs on a disk-usage list.
        assert!(!snapshot.filesystems.is_empty());
        assert!(snapshot.filesystems.iter().all(|fs| fs.fs_type != "tmpfs"), "{:?}", snapshot.filesystems);
        assert!(snapshot.disks.iter().all(|disk| !disk.device.starts_with("loop")));

        assert!(snapshot.memory.total_bytes > 0 && !snapshot.memory.estimated);
        assert!(snapshot.memory.used_bytes < snapshot.memory.total_bytes);
        // Nothing about this host is degraded, so nothing should be claimed.
        assert!(snapshot.warnings.is_empty(), "{:?}", snapshot.warnings);
    }

    #[test]
    fn starttime_comes_out_of_a_raw_stat_line_past_the_last_paren() {
        // Field 22 is starttime. comm holds a space and a ')' again, because the
        // kill guard reads the unprojected file straight off the host.
        let mut line = String::from("42 (evil ) name) ");
        for field in 3..=52 {
            line.push_str(&format!("{field} "));
        }
        assert_eq!(starttime_of(&line), Some(22));
        assert_eq!(starttime_of("not a stat line"), None);
        assert_eq!(starttime_of(""), None);
    }

    #[test]
    fn listening_sockets_parse_from_either_tool() {
        let ss = "tcp   LISTEN 0 4096  0.0.0.0:22  0.0.0.0:*  users:((\"sshd\",pid=800,fd=3))\n\
                  tcp   LISTEN 0 511   [::]:443    [::]:*     users:((\"nginx\",pid=1200,fd=6))\n\
                  udp   UNCONN 0 0     0.0.0.0:68  0.0.0.0:*\n\
                  tcp   LISTEN 0 4096            *:3306      *:*\n";
        let parsed = parse_ss(ss);
        assert_eq!(parsed.len(), 4);
        assert_eq!((parsed[0].address.as_str(), parsed[0].port.as_str()), ("0.0.0.0", "22"));
        assert_eq!(parsed[0].process.as_deref(), Some("sshd (800)"));
        // An IPv6 address must split at its last colon, not its first.
        assert_eq!((parsed[1].address.as_str(), parsed[1].port.as_str()), ("[::]", "443"));
        // No `users:` column at all is a listener we lack the privilege to name --
        // which is what unprivileged `ss` prints for every row.
        assert_eq!(parsed[2].process, None);
        // A wildcard bind has no host part in front of the colon.
        assert_eq!((parsed[3].address.as_str(), parsed[3].port.as_str()), ("*", "3306"));

        let netstat = "Active Internet connections (only servers)\n\
                       Proto Recv-Q Send-Q Local Address Foreign Address State PID/Program name\n\
                       tcp        0      0 0.0.0.0:22    0.0.0.0:*   LISTEN      800/sshd\n\
                       tcp6       0      0 :::443        :::*        LISTEN      -\n\
                       udp        0      0 0.0.0.0:68    0.0.0.0:*               700/dhclient\n";
        let parsed = parse_netstat(netstat);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].process.as_deref(), Some("sshd (800)"));
        // `-` is netstat's "you are not root", not a process named "-".
        assert_eq!(parsed[1].process, None);
        assert_eq!((parsed[2].protocol.as_str(), parsed[2].port.as_str()), ("udp", "68"));
        assert_eq!(parsed[2].process.as_deref(), Some("dhclient (700)"));
    }

    #[test]
    fn only_whole_block_devices_get_an_io_row() {
        let mut before = sample(0, 100.0, 0);
        let mut after = sample(0, 102.0, 0);
        // sda and sda1 both appear in diskstats; only sda is in /sys/block.
        for (sample, reads) in [(&mut before, 0u64), (&mut after, 4096u64)] {
            sample.disks = vec![
                ("sda".into(), reads, 0),
                ("sda1".into(), reads, 0),
                ("loop0".into(), reads, 0),
            ];
            sample.block_devices = vec!["sda".into(), "loop0".into()];
        }
        let disks = disk_io(Some(&before), &after, Some(2.0));
        assert_eq!(disks.len(), 1);
        assert_eq!(disks[0].device, "sda");
        // 4096 sectors of 512 bytes over two seconds.
        assert_eq!(disks[0].read_bytes_per_sec, 4096.0 * 512.0 / 2.0);
    }
}
