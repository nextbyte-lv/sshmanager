import { Globe } from "lucide-react";

import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { formatRate } from "@/lib/monitor";
import { cn } from "@/lib/utils";
import type { Connection } from "@/types/monitor";

interface ConnectionsTableProps {
    connections: Connection[];
    /** How many of them could not be attributed to a process, i.e. need root. */
    unattributed: number;
    elevated: boolean;
    measuring: boolean;
}

export function ConnectionsTable({ connections, unattributed, elevated, measuring }: ConnectionsTableProps) {
    // Busiest first: whatever is moving the most data is what you opened this for.
    // A public peer breaks ties, so an idle connection to the internet still floats
    // above idle loopback chatter.
    const rows = [...connections].sort((a, b) => {
        const rate = (c: Connection) => (c.rx_bytes_per_sec ?? 0) + (c.tx_bytes_per_sec ?? 0);
        const scope = (c: Connection) => (c.peer_scope === "public" ? 1 : 0);
        return rate(b) - rate(a) || scope(b) - scope(a) || a.peer.localeCompare(b.peer);
    });

    if (connections.length === 0) {
        return <p className="p-2 text-xs text-muted-foreground">No established connections.</p>;
    }

    return (
        <div className="flex h-full min-h-0 flex-col">
            <div className="min-h-0 flex-1 overflow-hidden [&>[data-slot=table-container]]:h-full">
                <Table className="table-fixed text-xs">
                    <TableHeader className="sticky top-0 z-10 bg-card">
                        <TableRow>
                            <TableHead className="h-7 w-40 px-2">Process</TableHead>
                            <TableHead className="h-7 w-14 px-2">Proto</TableHead>
                            <TableHead className="h-7 px-2">Peer</TableHead>
                            <TableHead className="h-7 w-40 px-2">Local</TableHead>
                            <TableHead className="h-7 w-24 px-2 text-right">Down</TableHead>
                            <TableHead className="h-7 w-24 px-2 text-right">Up</TableHead>
                        </TableRow>
                    </TableHeader>
                    <TableBody>
                        {rows.map((connection) => (
                            <TableRow key={`${connection.protocol}-${connection.local}-${connection.peer}`}>
                                <TableCell className="max-w-0 truncate px-2 py-0.5">
                                    {connection.process ? (
                                        <>
                                            {connection.process}
                                            <span className="ml-1 text-muted-foreground">{connection.pid}</span>
                                        </>
                                    ) : (
                                        <span
                                            className="text-muted-foreground"
                                            title={
                                                elevated
                                                    ? "Even with sudo this socket has no owning process — usually a kernel or NAT-relayed socket"
                                                    : `Owned by uid ${connection.uid ?? "?"}. Naming the process needs root — use the shield button.`
                                            }
                                        >
                                            uid {connection.uid ?? "?"}
                                        </span>
                                    )}
                                </TableCell>
                                <TableCell className="px-2 py-0.5 text-muted-foreground">
                                    {connection.protocol}
                                </TableCell>
                                <TableCell className="max-w-0 truncate px-2 py-0.5 font-mono">
                                    {/* The one thing worth spotting at a glance: an
                                        outbound connection to somewhere on the open
                                        internet. */}
                                    {connection.peer_scope === "public" && (
                                        <Globe
                                            className="mr-1 inline size-3 shrink-0 align-[-1px] text-warn"
                                            aria-label="Public address"
                                        >
                                            <title>This peer is on the public internet</title>
                                        </Globe>
                                    )}
                                    <span className={cn(connection.peer_scope === "public" && "text-warn")}>
                                        {connection.peer}
                                    </span>
                                </TableCell>
                                <TableCell className="max-w-0 truncate px-2 py-0.5 font-mono text-muted-foreground">
                                    {connection.local}
                                </TableCell>
                                <TableCell className="px-2 py-0.5 text-right font-mono tabular-nums">
                                    {measuring ? "—" : formatRate(connection.rx_bytes_per_sec ?? 0)}
                                </TableCell>
                                <TableCell className="px-2 py-0.5 text-right font-mono tabular-nums">
                                    {measuring ? "—" : formatRate(connection.tx_bytes_per_sec ?? 0)}
                                </TableCell>
                            </TableRow>
                        ))}
                    </TableBody>
                </Table>
            </div>

            <div className="shrink-0 space-y-0.5 border-t border-border px-2 py-1 text-[10px] text-muted-foreground">
                <p>
                    {connections.length} established {connections.length === 1 ? "connection" : "connections"}. Byte
                    counts come from the kernel's per-socket TCP statistics, so UDP shows no rate, and a connection
                    that opens and closes between two refreshes is not counted at all.
                </p>
                {unattributed > 0 && !elevated && (
                    <p className="text-warn">
                        {unattributed} of these belong to another user. Turn on the shield button to name the
                        processes behind them.
                    </p>
                )}
            </div>
        </div>
    );
}
