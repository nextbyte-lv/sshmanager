import type { ReactNode } from "react";

import { Sparkline } from "@/components/monitor/Sparkline";
import { Progress } from "@/components/ui/progress";
import { formatBytes, formatDuration, formatPercent, formatRate } from "@/lib/monitor";
import { cn } from "@/lib/utils";
import type { Snapshot } from "@/types/monitor";

export interface StatHistory {
    cpu: number[];
    memory: number[];
    rx: number[];
    tx: number[];
}

interface MonitorStatsProps {
    snapshot: Snapshot;
    history: StatHistory;
}

// The palette is monochrome by design, so a gauge only takes on colour once it is
// worth looking at. `--warn` and `--destructive` are the only two it ever uses.
function gaugeTone(percent: number): string {
    if (percent >= 90) return "[&_[data-slot=progress-indicator]]:bg-destructive";
    if (percent >= 75) return "[&_[data-slot=progress-indicator]]:bg-warn";
    return "";
}

function Card({ title, aside, children }: { title: string; aside?: ReactNode; children: ReactNode }) {
    return (
        <div className="min-w-44 flex-1 rounded-md border border-border p-2">
            <div className="flex items-baseline justify-between gap-2">
                <span className="text-[10px] uppercase tracking-wide text-muted-foreground">{title}</span>
                {aside}
            </div>
            {children}
        </div>
    );
}

/**
 * A labelled figure whose value sits in a fixed-width slot.
 *
 * Every number on this strip changes width as it changes — a device goes quiet and
 * `33.6 KB/s` becomes `—`, CPU crosses from `9%` to `10%` — and letting the text
 * size the box made each one shove its neighbours sideways on every refresh. `ch`
 * units on a monospace font give a slot exactly as wide as the longest value the
 * formatter can produce, at whatever font size the caller is using.
 *
 * `width` is a literal Tailwind class at each call site so the scanner still sees
 * it; the number in it is a character count, not pixels.
 */
function Metric({
    label,
    value,
    width,
    className,
}: {
    label: string;
    value: string;
    width: string;
    className?: string;
}) {
    return (
        <span className={cn("inline-flex items-baseline gap-1 font-mono", className)}>
            <span>{label}</span>
            <span className={cn("text-right tabular-nums", width)}>{value}</span>
        </span>
    );
}

/** Longest the byte-rate formatter goes is `1024 KB/s`. */
function Rate({ label, value }: { label: string; value: number }) {
    return <Metric label={label} value={formatRate(value)} width="w-[9ch]" />;
}

function Gauge({ percent, className }: { percent: number; className?: string }) {
    const safe = Number.isFinite(percent) ? Math.min(Math.max(percent, 0), 100) : 0;
    return <Progress value={safe} className={cn("mt-1.5", gaugeTone(safe), className)} />;
}

export function MonitorStats({ snapshot, history }: MonitorStatsProps) {
    const { cpu, memory, swap, host } = snapshot;
    const memoryPercent = memory.total_bytes ? (memory.used_bytes / memory.total_bytes) * 100 : 0;

    // Every interface summed. Unlike disk I/O, network counters don't nest, so a
    // total here is a real number rather than the same bytes counted twice.
    const rx = snapshot.network.reduce((sum, nic) => sum + nic.rx_bytes_per_sec, 0);
    const tx = snapshot.network.reduce((sum, nic) => sum + nic.tx_bytes_per_sec, 0);

    return (
        <div className="flex flex-wrap gap-2 border-b border-border p-2">
            <Card
                title="CPU"
                aside={
                    <span className="font-mono text-sm">
                        {cpu ? formatPercent(cpu.busy, 0) : "measuring…"}
                    </span>
                }
            >
                <div className="truncate text-[11px] text-muted-foreground" title={host.cpu_model}>
                    {host.cpu_model} · {host.cores} {host.cores === 1 ? "core" : "cores"}
                </div>
                <Gauge percent={cpu?.busy ?? 0} />
                <div className="mt-1.5 h-6 text-foreground">
                    <Sparkline values={history.cpu} max={100} className="h-full w-full" />
                </div>
                {cpu && (
                    <>
                        {/* A compact heat strip rather than one labelled bar per
                            core: this has to stay legible at 128 cores in a dock
                            a few hundred pixels tall. */}
                        <div className="mt-1.5 flex flex-wrap gap-px">
                            {cpu.per_core.map((load, index) => (
                                <span
                                    key={index}
                                    title={`Core ${index}: ${formatPercent(load, 0)}`}
                                    className="h-2.5 w-1.5 bg-primary"
                                    style={{ opacity: 0.15 + (Math.min(load, 100) / 100) * 0.85 }}
                                />
                            ))}
                        </div>
                        <div className="mt-1.5 flex flex-wrap gap-x-2 text-[10px] text-muted-foreground">
                            <Metric label="usr" value={formatPercent(cpu.user, 0)} width="w-[4ch]" />
                            <Metric label="sys" value={formatPercent(cpu.system, 0)} width="w-[4ch]" />
                            <Metric label="io" value={formatPercent(cpu.iowait, 0)} width="w-[4ch]" />
                            {/* Time the hypervisor gave to another tenant. On a
                                throttled VPS this one number is the whole answer. */}
                            <Metric
                                label="steal"
                                value={formatPercent(cpu.steal, 0)}
                                width="w-[4ch]"
                                className={cpu.steal >= 5 ? "text-warn" : undefined}
                            />
                        </div>
                    </>
                )}
            </Card>

            <Card
                title="Memory"
                aside={
                    <span className="font-mono text-sm">
                        {formatBytes(memory.used_bytes)} / {formatBytes(memory.total_bytes)}
                    </span>
                }
            >
                <div className="text-[11px] text-muted-foreground">
                    {formatBytes(memory.available_bytes)} available
                    {memory.estimated && " (estimated)"}
                </div>
                <Gauge percent={memoryPercent} />
                <div className="mt-1.5 h-6 text-foreground">
                    <Sparkline values={history.memory} max={100} className="h-full w-full" />
                </div>
                <div className="mt-1.5 flex flex-wrap gap-x-2 text-[10px] text-muted-foreground">
                    <Metric label="cache" value={formatBytes(memory.cache_bytes)} width="w-[8ch]" />
                    <Metric label="buffers" value={formatBytes(memory.buffers_bytes)} width="w-[8ch]" />
                </div>
                {swap && (
                    <div className="mt-1 text-[10px] text-muted-foreground">
                        swap {formatBytes(swap.used_bytes)} / {formatBytes(swap.total_bytes)}
                    </div>
                )}
            </Card>

            <Card
                title="Network"
                aside={
                    <span className="flex items-baseline gap-1.5 text-xs">
                        <Rate label="↓" value={rx} />
                        <Rate label="↑" value={tx} />
                    </span>
                }
            >
                <div className="mt-1 h-8 text-foreground">
                    <Sparkline values={history.rx} className="h-full w-full" />
                </div>
                <div className="mt-0.5 max-h-16 space-y-0.5 overflow-y-auto">
                    {snapshot.network.map((nic) => (
                        <div
                            key={nic.name}
                            className="flex items-baseline justify-between gap-2 text-[10px] text-muted-foreground"
                        >
                            <span className="truncate">{nic.name}</span>
                            <span className="flex shrink-0 items-baseline gap-1.5">
                                <Rate label="↓" value={nic.rx_bytes_per_sec} />
                                <Rate label="↑" value={nic.tx_bytes_per_sec} />
                            </span>
                        </div>
                    ))}
                </div>
            </Card>

            <Card
                title="Disks"
                aside={
                    <span className="text-[11px] text-muted-foreground">
                        up {formatDuration(host.uptime_seconds)} · load{" "}
                        {snapshot.load.map((value) => value.toFixed(2)).join(" ")}
                    </span>
                }
            >
                <div className="max-h-28 space-y-1 overflow-y-auto">
                    {snapshot.filesystems.map((fs) => (
                        <div key={fs.mount}>
                            <div className="flex justify-between gap-2 text-[11px]">
                                <span className="truncate" title={`${fs.device} (${fs.fs_type})`}>
                                    {fs.mount}
                                </span>
                                <span className="shrink-0 font-mono text-muted-foreground">
                                    {formatBytes(fs.used_bytes)} / {formatBytes(fs.total_bytes)}
                                </span>
                            </div>
                            <Gauge percent={fs.used_percent} className="mt-0.5" />
                        </div>
                    ))}
                    {snapshot.filesystems.length === 0 && (
                        <p className="text-[11px] text-muted-foreground">No local filesystems reported.</p>
                    )}
                </div>
                {/* Per device and never a total: /proc/diskstats lists sda, sda1
                    and dm-0 alike, so a sum would count the same bytes twice.

                    Every device is listed whether or not it is busy. Showing only
                    the active ones meant rows appearing and vanishing on each
                    refresh, which moved everything below them; a dash says "no
                    I/O" just as well and holds its place. */}
                {snapshot.disks.length > 0 && (
                    <div className="mt-1.5 max-h-20 space-y-0.5 overflow-y-auto">
                        {snapshot.disks.map((disk) => (
                            <div
                                key={disk.device}
                                className="flex items-baseline justify-between gap-2 text-[10px] text-muted-foreground"
                            >
                                <span className="truncate">{disk.device}</span>
                                <span className="flex shrink-0 items-baseline gap-1.5">
                                    <Rate label="r" value={disk.read_bytes_per_sec} />
                                    <Rate label="w" value={disk.write_bytes_per_sec} />
                                </span>
                            </div>
                        ))}
                    </div>
                )}
            </Card>

            {snapshot.pressure.length > 0 && (
                <Card title="Pressure">
                    <div className="space-y-1">
                        {snapshot.pressure.map(([resource, avg]) => (
                            <div key={resource}>
                                <div className="flex justify-between gap-2 text-[11px]">
                                    <span>{resource}</span>
                                    <span className="font-mono text-muted-foreground">{formatPercent(avg, 1)}</span>
                                </div>
                                <Gauge percent={avg} className="mt-0.5" />
                            </div>
                        ))}
                    </div>
                    <p className="mt-1 text-[10px] text-muted-foreground">
                        share of the last 10s something was stalled waiting
                    </p>
                </Card>
            )}
        </div>
    );
}
