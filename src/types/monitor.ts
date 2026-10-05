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
  /** Summed over this process's sockets. TCP only; null until there are two samples. */
  net_rx_bytes_per_sec: number | null;
  net_tx_bytes_per_sec: number | null;
  /**
   * What the kernel says is running, as opposed to what argv claims. Null for a
   * kernel thread, and for another user's process unless the lookup was elevated.
   */
  exe_path: string | null;
  /** The binary was unlinked after the process started. */
  exe_deleted: boolean;
  /** The binary lives in a world-writable directory. */
  exe_suspicious: boolean;
}

export type PeerScope = "loopback" | "private" | "public" | "unspecified";

export interface Connection {
  protocol: string;
  state: string;
  local: string;
  peer: string;
  peer_scope: PeerScope;
  pid: number | null;
  /** Null when the socket belongs to another user and the lookup was not elevated. */
  process: string | null;
  uid: number | null;
  rx_bytes_per_sec: number | null;
  tx_bytes_per_sec: number | null;
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
  connections: Connection[];
  /** Connections whose owning process could not be named — needs root. */
  unattributed_connections: number;
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

// Mirrors `src-tauri/src/ssh/dimms.rs`. Physical hardware, so unlike everything
// above it is read once on demand rather than sampled.

export interface MemoryModule {
  /** The slot's silkscreen name, e.g. `DIMM_A1`. */
  locator: string;
  bank: string | null;
  /** `DDR4`, `DDR5`, `LPDDR5`… `Unknown` on a hypervisor that fakes the table. */
  kind: string;
  size_bytes: number;
  /** Rated speed in MT/s. */
  speed_mts: number | null;
  /** What it is actually clocked at — lower than rated when the board down-clocks. */
  configured_mts: number | null;
  manufacturer: string | null;
  part_number: string | null;
  rank: number | null;
  form_factor: string | null;
  /** `Registered`, `Unbuffered`, `LRDIMM`, `Non-volatile`. */
  detail: string[];
}

export interface MemoryInventory {
  modules: MemoryModule[];
  /** Slots the firmware reports, populated or not. */
  total_slots: number;
  empty_slots: number;
  ecc: string | null;
  max_capacity_bytes: number | null;
  /** `edac` carries type and size only — the UI says so rather than showing blanks. */
  source: "dmi" | "edac" | null;
  warnings: string[];
}
