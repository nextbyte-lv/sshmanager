import { AlertTriangle, Maximize2, Minimize2, RefreshCw, Sparkles, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { ConfirmDialog } from "@/components/ConfirmDialog";
import { MonitorStats, type StatHistory } from "@/components/monitor/MonitorStats";
import { PortsTable } from "@/components/monitor/PortsTable";
import { ProcessTable, type CpuScale } from "@/components/monitor/ProcessTable";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import {
    DEFAULT_DIRECTION,
    filterProcesses,
    movedPids,
    pushHistory,
    sortProcesses,
    type SortColumn,
    type SortDirection,
} from "@/lib/monitor";
import { monitorKill, monitorPorts, monitorSample } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import type { KillSignal, ListeningSocket, Process, Snapshot } from "@/types/monitor";

/** Samples kept behind each sparkline. At the 2s default, two minutes of history. */
const HISTORY_LENGTH = 60;

/**
 * The table renders this many rows of the current sort. A busy host has thousands
 * of processes and no dock is tall enough to show them; capping the DOM is what
 * actually keeps the panel responsive.
 */
const ROW_CAP = 300;

const INTERVALS = [
    { value: "1000", label: "1s" },
    { value: "2000", label: "2s" },
    { value: "5000", label: "5s" },
    { value: "0", label: "Paused" },
];

const NO_FLASH: ReadonlySet<number> = new Set<number>();

const EMPTY_HISTORY: StatHistory = { cpu: [], memory: [], rx: [], tx: [] };

interface MonitorPanelProps {
    sessionId: string;
    /** Height of the dock in pixels, driven by the divider above it. */
    height: number;
    maximized: boolean;
    onToggleMaximized: () => void;
    onClose: () => void;
}

export function MonitorPanel({
    sessionId,
    height,
    maximized,
    onToggleMaximized,
    onClose,
}: MonitorPanelProps) {
    const panelRef = useRef<HTMLDivElement>(null);

    const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
    const [history, setHistory] = useState<StatHistory>(EMPTY_HISTORY);
    // Two error slots, never one: `listError` belongs to the poll and is cleared by
    // it, `actionError` belongs to whatever the user last asked for. Sharing a slot
    // means the poll -- which always runs last -- wipes the message the user needs.
    const [listError, setListError] = useState<string | null>(null);
    const [actionError, setActionError] = useState<string | null>(null);
    const [pollPaused, setPollPaused] = useState(false);
    const [intervalMs, setIntervalMs] = useState(2000);
    const [refreshKey, setRefreshKey] = useState(0);

    const [tab, setTab] = useState<"processes" | "ports">("processes");
    const [filter, setFilter] = useState("");
    const [sortColumn, setSortColumn] = useState<SortColumn>("cpu");
    const [sortDirection, setSortDirection] = useState<SortDirection>("desc");
    const [cpuScale, setCpuScale] = useState<CpuScale>("core");
    const [flashEnabled, setFlashEnabled] = useState(true);
    const [flashPids, setFlashPids] = useState<ReadonlySet<number>>(NO_FLASH);
    const orderRef = useRef<{ key: string; pids: number[] }>({ key: "", pids: [] });

    const [ports, setPorts] = useState<ListeningSocket[] | null>(null);
    const [portsError, setPortsError] = useState<string | null>(null);
    const [killTarget, setKillTarget] = useState<{ process: Process; signal: KillSignal } | null>(null);

    const applySnapshot = useCallback((next: Snapshot) => {
        setSnapshot(next);
        setHistory((current) => ({
            // Rates are unknown on a first sample, not zero. Charting the zero
            // would draw a trough that never happened, so those series simply
            // don't gain a point until there is something real to plot.
            cpu: next.cpu ? pushHistory(current.cpu, next.cpu.busy, HISTORY_LENGTH) : current.cpu,
            memory: pushHistory(
                current.memory,
                next.memory.total_bytes ? (next.memory.used_bytes / next.memory.total_bytes) * 100 : 0,
                HISTORY_LENGTH,
            ),
            rx: next.measuring
                ? current.rx
                : pushHistory(
                      current.rx,
                      next.network.reduce((sum, nic) => sum + nic.rx_bytes_per_sec, 0),
                      HISTORY_LENGTH,
                  ),
            tx: next.measuring
                ? current.tx
                : pushHistory(
                      current.tx,
                      next.network.reduce((sum, nic) => sum + nic.tx_bytes_per_sec, 0),
                      HISTORY_LENGTH,
                  ),
        }));
    }, []);

    useEffect(() => {
        let cancelled = false;
        let timer: number | undefined;

        function schedule() {
            if (cancelled || intervalMs === 0) return;
            timer = window.setTimeout(() => void tick(), intervalMs);
        }

        async function tick() {
            // An inactive tab's panes are hidden with `display:none` and never
            // unmounted, so `offsetParent` is null exactly when this panel is
            // off-screen. Without this gate, every panel ever opened would keep
            // polling its host forever.
            if (panelRef.current && panelRef.current.offsetParent === null) {
                setPollPaused(true);
                schedule();
                return;
            }
            setPollPaused(false);
            try {
                const next = await monitorSample(sessionId);
                if (cancelled) return;
                applySnapshot(next);
                setListError(null);
            } catch (error) {
                if (!cancelled) setListError(String(error));
            }
            // Chained after the await rather than a bare `setInterval`: a slow host
            // slows the poll rate instead of queueing up samples behind itself.
            schedule();
        }

        void tick();
        return () => {
            cancelled = true;
            if (timer !== undefined) window.clearTimeout(timer);
        };
    }, [sessionId, intervalMs, refreshKey, applySnapshot]);

    const loadPorts = useCallback(async () => {
        setPortsError(null);
        try {
            setPorts(await monitorPorts(sessionId));
        } catch (error) {
            setPortsError(String(error));
        }
    }, [sessionId]);

    // Listening sockets change rarely, so they are fetched when the tab opens and
    // on an explicit refresh rather than on the sample timer.
    useEffect(() => {
        if (tab === "ports") void loadPorts();
    }, [tab, refreshKey, loadPorts]);

    const rows = useMemo(() => {
        if (!snapshot) return [];
        return sortProcesses(filterProcesses(snapshot.processes, filter), sortColumn, sortDirection);
    }, [snapshot, filter, sortColumn, sortDirection]);

    const visible = useMemo(() => rows.slice(0, ROW_CAP), [rows]);

    useEffect(() => {
        const key = `${sortColumn}:${sortDirection}:${filter}`;
        const pids = visible.map((process) => process.pid);
        const previous = orderRef.current;
        orderRef.current = { key, pids };

        // A changed sort or filter reorders everything at once, and flashing the
        // whole table says nothing -- so that tick is skipped instead.
        if (!flashEnabled || previous.key !== key || previous.pids.length === 0) {
            setFlashPids((current) => (current.size === 0 ? current : NO_FLASH));
            return;
        }

        const moved = movedPids(previous.pids, pids);
        setFlashPids((current) => (moved.size === 0 && current.size === 0 ? current : moved));
        if (moved.size === 0) return;

        // Cleared a beat later, so the browser paints the flash before the row's
        // colour transition fades it out. Doing both in one paint shows nothing.
        const handle = window.setTimeout(
            () => setFlashPids((current) => (current.size === 0 ? current : NO_FLASH)),
            90,
        );
        return () => window.clearTimeout(handle);
    }, [visible, sortColumn, sortDirection, filter, flashEnabled]);

    function handleSort(column: SortColumn) {
        if (column === sortColumn) {
            setSortDirection((direction) => (direction === "asc" ? "desc" : "asc"));
            return;
        }
        setSortColumn(column);
        setSortDirection(DEFAULT_DIRECTION[column]);
    }

    // Every await in a click handler needs a catch that reaches the screen: in a
    // desktop webview there is no console anyone is watching.
    async function copyToClipboard(text: string) {
        setActionError(null);
        try {
            await navigator.clipboard.writeText(text);
        } catch (error) {
            setActionError(`could not copy to the clipboard: ${error}`);
        }
    }

    async function signalTarget() {
        if (!killTarget) return;
        setActionError(null);
        try {
            // `start_ticks` is the process identity the row was drawn from; the
            // backend refuses the signal if the pid has since been recycled.
            await monitorKill(
                sessionId,
                killTarget.process.pid,
                killTarget.process.start_ticks,
                killTarget.signal,
            );
            setRefreshKey((key) => key + 1);
        } catch (error) {
            setActionError(String(error));
        }
    }

    const signalLabel: Record<KillSignal, string> = {
        term: "End process",
        kill: "Force kill",
        int: "Interrupt",
        hup: "Reload",
    };

    return (
        <div
            ref={panelRef}
            className="flex min-h-0 shrink-0 flex-col border-t border-border bg-card"
            style={maximized ? { flex: "1 1 0%" } : { height }}
        >
            <div className="flex shrink-0 flex-wrap items-center gap-1 border-b border-border p-1.5">
                <Button
                    size="xs"
                    variant={tab === "processes" ? "secondary" : "ghost"}
                    onClick={() => setTab("processes")}
                >
                    Processes
                </Button>
                <Button
                    size="xs"
                    variant={tab === "ports" ? "secondary" : "ghost"}
                    onClick={() => setTab("ports")}
                >
                    Ports
                </Button>

                {tab === "processes" && (
                    <Input
                        value={filter}
                        onChange={(e) => setFilter(e.target.value)}
                        placeholder="Filter by name, user or PID"
                        className="h-6 w-52 text-xs"
                    />
                )}

                <div className="ml-auto flex items-center gap-1">
                    {snapshot?.measuring && (
                        <span className="text-[10px] text-muted-foreground">measuring…</span>
                    )}
                    {pollPaused && (
                        <span
                            className="text-[10px] text-muted-foreground"
                            title="This tab is in the background, so sampling is paused"
                        >
                            paused
                        </span>
                    )}

                    {tab === "processes" && (
                        <Button
                            size="xs"
                            variant="ghost"
                            title={
                                cpuScale === "core"
                                    ? "CPU is shown per core, as htop does: 100% is one core saturated. Click for the whole-machine scale."
                                    : "CPU is shown across the whole machine, as Windows Task Manager does. Click for the per-core scale."
                            }
                            onClick={() => setCpuScale((scale) => (scale === "core" ? "machine" : "core"))}
                        >
                            {cpuScale === "core" ? "per core" : "per machine"}
                        </Button>
                    )}

                    {tab === "processes" && (
                        <Button
                            size="icon-xs"
                            variant={flashEnabled ? "secondary" : "ghost"}
                            title="Flash rows that moved or appeared since the last refresh"
                            onClick={() => setFlashEnabled((enabled) => !enabled)}
                        >
                            <Sparkles />
                        </Button>
                    )}

                    <Select value={String(intervalMs)} onValueChange={(value) => setIntervalMs(Number(value))}>
                        <SelectTrigger className="h-6 w-24 text-xs">
                            <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                            {INTERVALS.map((option) => (
                                <SelectItem key={option.value} value={option.value}>
                                    {option.label}
                                </SelectItem>
                            ))}
                        </SelectContent>
                    </Select>

                    <Button
                        size="icon-xs"
                        variant="ghost"
                        title="Refresh now"
                        onClick={() => setRefreshKey((key) => key + 1)}
                    >
                        <RefreshCw />
                    </Button>
                    <Button
                        size="icon-xs"
                        variant="ghost"
                        title={maximized ? "Restore the terminal" : "Fill the pane"}
                        onClick={onToggleMaximized}
                    >
                        {maximized ? <Minimize2 /> : <Maximize2 />}
                    </Button>
                    <Button size="icon-xs" variant="ghost" title="Close the monitor" onClick={onClose}>
                        <X />
                    </Button>
                </div>
            </div>

            {(listError || actionError) && (
                <div className="shrink-0 space-y-1 border-b border-border px-2 py-1">
                    {listError && <p className="text-xs text-destructive">{listError}</p>}
                    {actionError && <p className="text-xs text-destructive">{actionError}</p>}
                </div>
            )}

            {snapshot && snapshot.warnings.length > 0 && (
                <div className="shrink-0 space-y-0.5 border-b border-border px-2 py-1">
                    {snapshot.warnings.map((warning) => (
                        <p key={warning} className="flex items-start gap-1.5 text-[11px] text-warn">
                            <AlertTriangle className="mt-0.5 size-3 shrink-0" />
                            <span>{warning}</span>
                        </p>
                    ))}
                </div>
            )}

            {!snapshot && !listError && (
                <p className="p-2 text-xs text-muted-foreground">Reading the host…</p>
            )}

            {snapshot && (
                <>
                    <div className={cn("shrink-0 overflow-y-auto", maximized ? "max-h-72" : "max-h-52")}>
                        <MonitorStats snapshot={snapshot} history={history} />
                    </div>
                    <div className="min-h-0 flex-1">
                        {tab === "processes" ? (
                            <ProcessTable
                                rows={visible}
                                total={snapshot.process_count}
                                shown={visible.length}
                                sampledAt={snapshot.sampled_at}
                                cores={snapshot.host.cores}
                                cpuScale={cpuScale}
                                sortColumn={sortColumn}
                                sortDirection={sortDirection}
                                onSort={handleSort}
                                flashPids={flashPids}
                                onSignal={(process, signal) => setKillTarget({ process, signal })}
                                onCopy={(text) => void copyToClipboard(text)}
                            />
                        ) : (
                            <PortsTable sockets={ports} loading={ports === null && !portsError} error={portsError} />
                        )}
                    </div>
                </>
            )}

            <ConfirmDialog
                open={killTarget !== null}
                onOpenChange={(open) => !open && setKillTarget(null)}
                title={killTarget ? `${signalLabel[killTarget.signal]} ${killTarget.process.name}?` : ""}
                description={
                    killTarget && (
                        <>
                            PID {killTarget.process.pid} owned by {killTarget.process.user || "an unknown user"}.
                            {killTarget.signal === "kill"
                                ? " SIGKILL cannot be caught, so the process gets no chance to save anything or clean up."
                                : " The process is asked to stop and may refuse."}{" "}
                            If it is not yours, this is retried with sudo.
                        </>
                    )
                }
                confirmLabel={killTarget ? signalLabel[killTarget.signal] : "Confirm"}
                pendingLabel="Signalling…"
                destructive
                onConfirm={signalTarget}
            />
        </div>
    );
}
