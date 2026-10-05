import {
    Dialog,
    DialogContent,
    DialogDescription,
    DialogHeader,
    DialogTitle,
} from "@/components/ui/dialog";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { formatBytes } from "@/lib/monitor";
import type { MemoryInventory, MemoryModule } from "@/types/monitor";

interface MemoryModulesDialogProps {
    open: boolean;
    onOpenChange: (open: boolean) => void;
    inventory: MemoryInventory;
}

/**
 * A module's speed, and the fact that it is not running at the one it was sold
 * at. A fully populated board dropping four DDR5-6000 sticks to 3600 is invisible
 * in every other view here and is exactly what someone opens this table to find.
 */
function Speed({ module }: { module: MemoryModule }) {
    const configured = module.configured_mts ?? module.speed_mts;
    if (!configured) return <span className="text-muted-foreground">—</span>;

    const rated = module.speed_mts;
    const throttled = rated !== null && rated > configured;
    return (
        <span className={throttled ? "text-warn" : undefined}>
            {configured} MT/s
            {throttled && (
                <span className="text-muted-foreground" title="The board is not running this module at its rated speed">
                    {" "}
                    of {rated}
                </span>
            )}
        </span>
    );
}

export function MemoryModulesDialog({ open, onOpenChange, inventory }: MemoryModulesDialogProps) {
    const total = inventory.modules.reduce((sum, module) => sum + module.size_bytes, 0);

    return (
        <Dialog open={open} onOpenChange={onOpenChange}>
            <DialogContent className="sm:max-w-3xl">
                <DialogHeader>
                    <DialogTitle>Memory modules</DialogTitle>
                    <DialogDescription>
                        {formatBytes(total, 0)} in {inventory.modules.length}{" "}
                        {inventory.modules.length === 1 ? "module" : "modules"}
                        {inventory.total_slots > 0 && ` across ${inventory.total_slots} slots`}
                        {inventory.empty_slots > 0 && ` (${inventory.empty_slots} free)`}
                        {inventory.ecc && ` · ${inventory.ecc === "None" ? "no ECC" : inventory.ecc}`}
                        {inventory.max_capacity_bytes !== null &&
                            ` · ${formatBytes(inventory.max_capacity_bytes, 0)} maximum`}
                    </DialogDescription>
                </DialogHeader>

                {/* The table is the only thing here that can outgrow the dialog, so
                    the scroll container goes around it rather than on the content. */}
                <div className="max-h-[60vh] overflow-auto">
                    <Table>
                        <TableHeader>
                            <TableRow>
                                <TableHead>Slot</TableHead>
                                <TableHead>Size</TableHead>
                                <TableHead>Type</TableHead>
                                <TableHead>Speed</TableHead>
                                <TableHead>Rank</TableHead>
                                <TableHead>Manufacturer</TableHead>
                                <TableHead>Part number</TableHead>
                            </TableRow>
                        </TableHeader>
                        <TableBody>
                            {inventory.modules.map((module, index) => (
                                <TableRow key={`${module.locator}-${index}`}>
                                    <TableCell className="font-mono" title={module.bank ?? undefined}>
                                        {module.locator}
                                    </TableCell>
                                    <TableCell className="tabular-nums">
                                        {module.size_bytes > 0 ? formatBytes(module.size_bytes, 0) : "—"}
                                    </TableCell>
                                    <TableCell>
                                        {module.kind}
                                        {(module.form_factor || module.detail.length > 0) && (
                                            <span className="text-muted-foreground">
                                                {" "}
                                                {[module.form_factor, ...module.detail]
                                                    .filter(Boolean)
                                                    .join(" · ")}
                                            </span>
                                        )}
                                    </TableCell>
                                    <TableCell className="tabular-nums">
                                        <Speed module={module} />
                                    </TableCell>
                                    <TableCell className="tabular-nums">
                                        {module.rank !== null ? `${module.rank}R` : "—"}
                                    </TableCell>
                                    <TableCell>{module.manufacturer ?? "—"}</TableCell>
                                    <TableCell className="font-mono">{module.part_number ?? "—"}</TableCell>
                                </TableRow>
                            ))}
                        </TableBody>
                    </Table>
                </div>

                {inventory.warnings.map((warning) => (
                    <p key={warning} className="text-[11px] text-muted-foreground">
                        {warning}
                    </p>
                ))}
            </DialogContent>
        </Dialog>
    );
}
