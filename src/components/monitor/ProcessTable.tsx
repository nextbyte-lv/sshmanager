import { MoreVertical } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
    DropdownMenu,
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuSeparator,
    DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { formatAge, formatBytes, formatPercent, type SortColumn, type SortDirection } from "@/lib/monitor";
import { cn } from "@/lib/utils";
import type { KillSignal, Process } from "@/types/monitor";

export type CpuScale = "core" | "machine";

interface ProcessTableProps {
    rows: Process[];
    /** Everything the host reported, before the filter and the row cap. */
    total: number;
    shown: number;
    sampledAt: number;
    cores: number;
    cpuScale: CpuScale;
    sortColumn: SortColumn;
    sortDirection: SortDirection;
    onSort: (column: SortColumn) => void;
    flashPids: ReadonlySet<number>;
    onSignal: (process: Process, signal: KillSignal) => void;
    onCopy: (text: string) => void;
}

const COLUMNS: { column: SortColumn; label: string; className: string; title?: string }[] = [
    { column: "pid", label: "PID", className: "w-16 text-right" },
    { column: "user", label: "User", className: "w-24" },
    {
        column: "cpu",
        label: "CPU",
        className: "w-20 text-right",
        title: "Share of processor time since the last refresh",
    },
    { column: "memory", label: "Memory", className: "w-24 text-right" },
    { column: "threads", label: "Thr", className: "w-12 text-right" },
    { column: "started", label: "Age", className: "w-16 text-right" },
    { column: "name", label: "Command", className: "min-w-0" },
];

export function ProcessTable({
    rows,
    total,
    shown,
    sampledAt,
    cores,
    cpuScale,
    sortColumn,
    sortDirection,
    onSort,
    flashPids,
    onSignal,
    onCopy,
}: ProcessTableProps) {
    // Stored per-core (htop's and top's scale), divided down on request for the
    // Windows Task Manager reading where everything sums to 100.
    const scaleCpu = (value: number | null) =>
        value === null ? null : cpuScale === "core" ? value : value / Math.max(cores, 1);

    return (
        <div className="flex h-full min-h-0 flex-col">
            <div className="min-h-0 flex-1 overflow-hidden [&>[data-slot=table-container]]:h-full">
                <Table className="table-fixed text-xs">
                    <TableHeader className="sticky top-0 z-10 bg-card">
                        <TableRow>
                            {COLUMNS.map(({ column, label, className, title }) => (
                                <TableHead key={column} className={cn("h-7 px-2", className)}>
                                    <button
                                        type="button"
                                        className="cursor-pointer hover:text-foreground"
                                        title={title ?? `Sort by ${label.toLowerCase()}`}
                                        onClick={() => onSort(column)}
                                    >
                                        {label}
                                        {sortColumn === column && (sortDirection === "asc" ? " ▲" : " ▼")}
                                    </button>
                                </TableHead>
                            ))}
                            <TableHead className="h-7 w-8 px-1" />
                        </TableRow>
                    </TableHeader>
                    <TableBody>
                        {rows.map((process) => {
                            const flash = flashPids.has(process.pid);
                            return (
                                <TableRow
                                    key={process.pid}
                                    // The flash is painted with no transition and
                                    // cleared with one, so the row lights up at
                                    // once and fades out over a second. Both
                                    // classes carrying a transition would make the
                                    // arrival a slow pulse instead of a signal.
                                    className={cn(
                                        "group",
                                        flash ? "bg-flash/30" : "bg-transparent transition-colors duration-1000",
                                    )}
                                >
                                    <TableCell className="px-2 py-0.5 text-right font-mono text-muted-foreground">
                                        {process.pid}
                                    </TableCell>
                                    <TableCell className="max-w-24 truncate px-2 py-0.5" title={process.user}>
                                        {process.user || "—"}
                                    </TableCell>
                                    <TableCell className="px-2 py-0.5 text-right font-mono tabular-nums">
                                        {formatPercent(scaleCpu(process.cpu_percent))}
                                    </TableCell>
                                    <TableCell
                                        className="px-2 py-0.5 text-right font-mono tabular-nums"
                                        title={`${formatPercent(process.memory_percent)} of RAM`}
                                    >
                                        {formatBytes(process.memory_bytes)}
                                    </TableCell>
                                    <TableCell className="px-2 py-0.5 text-right font-mono text-muted-foreground">
                                        {process.threads}
                                    </TableCell>
                                    <TableCell className="px-2 py-0.5 text-right font-mono text-muted-foreground">
                                        {formatAge(process.started_at, sampledAt)}
                                    </TableCell>
                                    <TableCell className="max-w-0 truncate px-2 py-0.5" title={process.command}>
                                        {process.command}
                                    </TableCell>
                                    <TableCell className="px-1 py-0.5">
                                        <DropdownMenu>
                                            <DropdownMenuTrigger
                                                render={
                                                    <Button
                                                        size="icon-xs"
                                                        variant="ghost"
                                                        title="Process actions"
                                                        className="opacity-0 group-hover:opacity-100 aria-expanded:opacity-100"
                                                    />
                                                }
                                            >
                                                <MoreVertical />
                                            </DropdownMenuTrigger>
                                            <DropdownMenuContent align="end">
                                                {/* Command lines are truncated on
                                                    the host to keep the sample
                                                    small, so copying gives you
                                                    what is on screen. */}
                                                <DropdownMenuItem onClick={() => onCopy(process.command)}>
                                                    Copy command line
                                                </DropdownMenuItem>
                                                <DropdownMenuItem onClick={() => onCopy(String(process.pid))}>
                                                    Copy PID
                                                </DropdownMenuItem>
                                                <DropdownMenuSeparator />
                                                <DropdownMenuItem onClick={() => onSignal(process, "term")}>
                                                    End process (SIGTERM)
                                                </DropdownMenuItem>
                                                <DropdownMenuItem onClick={() => onSignal(process, "hup")}>
                                                    Reload (SIGHUP)
                                                </DropdownMenuItem>
                                                <DropdownMenuItem onClick={() => onSignal(process, "kill")}>
                                                    Force kill (SIGKILL)
                                                </DropdownMenuItem>
                                            </DropdownMenuContent>
                                        </DropdownMenu>
                                    </TableCell>
                                </TableRow>
                            );
                        })}
                    </TableBody>
                </Table>

                {rows.length === 0 && (
                    <p className="p-2 text-xs text-muted-foreground">No processes match this filter.</p>
                )}
            </div>

            <div className="flex shrink-0 items-center gap-2 border-t border-border px-2 py-1 text-[10px] text-muted-foreground">
                <span>
                    {shown === total
                        ? `${total} processes`
                        : `showing ${shown} of ${total} processes`}
                </span>
                <span>·</span>
                <span>
                    CPU shown {cpuScale === "core" ? "per core (100% = one core, as htop)" : "across the whole machine"}
                </span>
            </div>
        </div>
    );
}
