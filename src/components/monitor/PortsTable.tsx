import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import type { ListeningSocket } from "@/types/monitor";

interface PortsTableProps {
    sockets: ListeningSocket[] | null;
    loading: boolean;
    error: string | null;
}

export function PortsTable({ sockets, loading, error }: PortsTableProps) {
    if (error) {
        return <p className="p-2 text-xs text-destructive">{error}</p>;
    }
    if (loading || sockets === null) {
        return <p className="p-2 text-xs text-muted-foreground">Reading listening sockets…</p>;
    }
    if (sockets.length === 0) {
        return <p className="p-2 text-xs text-muted-foreground">Nothing is listening.</p>;
    }

    // Naming the process behind a socket needs root, so some rows legitimately
    // have no owner. Saying that beats a blank cell that reads as a bug.
    const anonymous = sockets.filter((socket) => socket.process === null).length;

    return (
        <div className="flex h-full min-h-0 flex-col">
            <div className="min-h-0 flex-1 overflow-hidden [&>[data-slot=table-container]]:h-full">
                <Table className="table-fixed text-xs">
                    <TableHeader className="sticky top-0 z-10 bg-card">
                        <TableRow>
                            <TableHead className="h-7 w-20 px-2">Proto</TableHead>
                            <TableHead className="h-7 w-16 px-2 text-right">Port</TableHead>
                            <TableHead className="h-7 px-2">Address</TableHead>
                            <TableHead className="h-7 px-2">Process</TableHead>
                        </TableRow>
                    </TableHeader>
                    <TableBody>
                        {sockets.map((socket, index) => (
                            <TableRow key={`${socket.protocol}-${socket.address}-${socket.port}-${index}`}>
                                <TableCell className="px-2 py-0.5">{socket.protocol}</TableCell>
                                <TableCell className="px-2 py-0.5 text-right font-mono">{socket.port}</TableCell>
                                <TableCell className="max-w-0 truncate px-2 py-0.5 font-mono text-muted-foreground">
                                    {socket.address}
                                </TableCell>
                                <TableCell className="max-w-0 truncate px-2 py-0.5">
                                    {socket.process ?? <span className="text-muted-foreground">not visible</span>}
                                </TableCell>
                            </TableRow>
                        ))}
                    </TableBody>
                </Table>
            </div>

            {anonymous > 0 && (
                <p className="shrink-0 border-t border-border px-2 py-1 text-[10px] text-muted-foreground">
                    {anonymous} of these belong to another user — naming them needs root.
                </p>
            )}
        </div>
    );
}
