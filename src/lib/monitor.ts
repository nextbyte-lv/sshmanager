// Pure helpers for the host monitor panel: formatting, sorting, and the rule that
// decides which rows flash on a refresh. Kept out of the components so the flash
// rule in particular lives in one readable place instead of inside a render.

import type { Process } from "@/types/monitor";

const BYTE_UNITS = ["B", "KB", "MB", "GB", "TB", "PB"];

export function formatBytes(bytes: number, fractionDigits = 1): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const exponent = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), BYTE_UNITS.length - 1);
  const value = bytes / 1024 ** exponent;
  // Whole bytes never want a decimal point, and neither does a three-digit value.
  const digits = exponent === 0 ? 0 : value >= 100 ? 0 : fractionDigits;
  return `${value.toFixed(digits)} ${BYTE_UNITS[exponent]}`;
}

export function formatRate(bytesPerSecond: number): string {
  if (!Number.isFinite(bytesPerSecond) || bytesPerSecond < 1) return "—";
  return `${formatBytes(bytesPerSecond)}/s`;
}

export function formatPercent(value: number | null, fractionDigits = 1): string {
  if (value === null || !Number.isFinite(value)) return "—";
  return `${value.toFixed(fractionDigits)}%`;
}

export function formatDuration(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return "—";
  const days = Math.floor(seconds / 86400);
  const hours = Math.floor((seconds % 86400) / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  if (days > 0) return `${days}d ${hours}h`;
  if (hours > 0) return `${hours}h ${minutes}m`;
  return `${minutes}m`;
}

/**
 * How long ago a process started, measured against the sample's own *remote*
 * clock. Using the local clock would be wrong by however far the two machines
 * disagree, which on a VM that has been suspended can be a lot.
 */
export function formatAge(startedAt: number | null, sampledAt: number): string {
  if (startedAt === null || !sampledAt) return "—";
  return formatDuration(sampledAt - startedAt);
}

export type SortColumn = "pid" | "user" | "name" | "cpu" | "memory" | "threads" | "started";
export type SortDirection = "asc" | "desc";

/** The direction each column should take on its first click, i.e. the interesting end first. */
export const DEFAULT_DIRECTION: Record<SortColumn, SortDirection> = {
  pid: "asc",
  user: "asc",
  name: "asc",
  cpu: "desc",
  memory: "desc",
  threads: "desc",
  started: "desc",
};

function compare(a: Process, b: Process, column: SortColumn): number {
  switch (column) {
    case "pid":
      return a.pid - b.pid;
    case "user":
      return a.user.localeCompare(b.user);
    case "name":
      return a.name.localeCompare(b.name);
    case "cpu":
      // A process whose CPU is not yet measurable sorts as zero rather than
      // floating to the top of a descending sort on the first tick.
      return (a.cpu_percent ?? 0) - (b.cpu_percent ?? 0);
    case "memory":
      return a.memory_bytes - b.memory_bytes;
    case "threads":
      return a.threads - b.threads;
    case "started":
      return (a.started_at ?? 0) - (b.started_at ?? 0);
  }
}

export function sortProcesses(
  processes: Process[],
  column: SortColumn,
  direction: SortDirection,
): Process[] {
  const sign = direction === "asc" ? 1 : -1;
  // Pid breaks every tie, so equal rows (a screenful of idle processes all at
  // 0.0%) hold a stable order instead of shuffling — and therefore never flash.
  return [...processes].sort((a, b) => sign * compare(a, b, column) || a.pid - b.pid);
}

export function filterProcesses(processes: Process[], query: string): Process[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return processes;
  return processes.filter(
    (process) =>
      process.name.toLowerCase().includes(needle) ||
      process.command.toLowerCase().includes(needle) ||
      process.user.toLowerCase().includes(needle) ||
      String(process.pid).includes(needle),
  );
}

/**
 * Which rows should flash: those whose position in the list changed, and those
 * that were not in it at all a moment ago. A row that holds its position never
 * flashes, however much its numbers moved.
 *
 * Callers must skip this on a tick where the sort or the filter changed — that
 * reorders everything at once, and flashing the whole table says nothing.
 */
export function movedPids(previousOrder: number[], nextOrder: number[]): Set<number> {
  const wasAt = new Map(previousOrder.map((pid, index) => [pid, index]));
  const moved = new Set<number>();
  nextOrder.forEach((pid, index) => {
    const before = wasAt.get(pid);
    if (before === undefined || before !== index) moved.add(pid);
  });
  return moved;
}

/** Rolling window of samples behind each sparkline. */
export function pushHistory(history: number[], value: number, limit: number): number[] {
  const next = [...history, value];
  return next.length > limit ? next.slice(next.length - limit) : next;
}
