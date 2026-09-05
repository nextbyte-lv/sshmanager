// Mirrors the payload built by `src-tauri/src/ssh/monitor.rs`.

export interface HostInfo {
  os: string;
  kernel: string;
  arch: string;
  cpu_model: string;
  cores: number;
  boot_time: number | null;
  uptime_seconds: number;
}

export interface CpuUsage {
  busy: number;
  user: number;
  system: number;
  iowait: number;
  /** Time the hypervisor gave to someone else. On a throttled VPS this is the diagnosis. */
  steal: number;
  per_core: number[];
}

export interface MemoryUsage {
  total_bytes: number;
  used_bytes: number;
  available_bytes: number;
  buffers_bytes: number;
  cache_bytes: number;
  /** `MemAvailable` was missing, so `used_bytes` is approximate. */
  estimated: boolean;
}

export interface SwapUsage {
  total_bytes: number;
  used_bytes: number;
}

export interface Filesystem {
  device: string;
  mount: string;
  fs_type: string;
  total_bytes: number;
  used_bytes: number;
  available_bytes: number;
  /** `used / (used + available)` — what `df` itself prints. */
  used_percent: number;
}

export interface NetInterface {
  name: string;
  rx_bytes_per_sec: number;
  tx_bytes_per_sec: number;
  rx_total: number;
  tx_total: number;
}

export interface DiskIo {
  device: string;
  read_bytes_per_sec: number;
  write_bytes_per_sec: number;
}

export interface Process {
  pid: number;
  ppid: number;
  user: string;
  name: string;
  command: string;
  state: string;
  threads: number;
  /** Per-core scale, as htop and `top` report it: four busy cores read 400. */
  cpu_percent: number | null;
  memory_bytes: number;
  memory_percent: number;
  started_at: number | null;
  /** `starttime` in clock ticks, passed back to a kill so it can prove identity. */
  start_ticks: number;
}

export interface Snapshot {
  host: HostInfo;
  /** Null on the first sample of a session — a rate needs two. */
  cpu: CpuUsage | null;
  memory: MemoryUsage;
  swap: SwapUsage | null;
  load: [number, number, number];
  /** `[resource, avg10]` pressure-stall pairs; empty before kernel 4.20. */
  pressure: [string, number][];
  filesystems: Filesystem[];
  network: NetInterface[];
  disks: DiskIo[];
  processes: Process[];
  /** Reasons a number on screen may not mean what it appears to. */
  warnings: string[];
  measuring: boolean;
  sampled_at: number;
  process_count: number;
}

export interface ListeningSocket {
  protocol: string;
  address: string;
  port: string;
  /** Null when naming the listener would have needed root. */
  process: string | null;
}

export type KillSignal = "term" | "kill" | "int" | "hup";
