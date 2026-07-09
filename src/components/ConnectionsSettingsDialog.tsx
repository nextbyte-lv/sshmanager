import { useState } from "react";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { Download, Upload } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { ScrollArea } from "@/components/ui/scroll-area";
import { ExportConnectionsDialog } from "@/components/ExportConnectionsDialog";
import { importConnections } from "@/lib/tauri";
import type { ConnectionProfile } from "@/types/connection";

interface ConnectionsSettingsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  connections: ConnectionProfile[];
  onImported: () => void;
}

export function ConnectionsSettingsDialog({
  open,
  onOpenChange,
  connections,
  onImported,
}: ConnectionsSettingsDialogProps) {
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [exportScope, setExportScope] = useState<{ ids: string[] | null; count: number } | null>(null);

  const allSelected = connections.length > 0 && selectedIds.size === connections.length;

  function toggleSelected(id: string, checked: boolean) {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (checked) next.add(id);
      else next.delete(id);
      return next;
    });
  }

  function toggleAll(checked: boolean) {
    setSelectedIds(checked ? new Set(connections.map((c) => c.id)) : new Set());
  }

  async function handleImportClick() {
    const path = await openFileDialog({
      multiple: false,
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (typeof path !== "string") return;
    try {
      const imported = await importConnections(path);
      onImported();
      window.alert(`Imported ${imported.length} connection${imported.length === 1 ? "" : "s"}.`);
    } catch (err) {
      window.alert(`Import failed: ${err}`);
    }
  }

  return (
    <>
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent className="sm:max-w-lg">
          <DialogHeader>
            <DialogTitle>Import &amp; export connections</DialogTitle>
          </DialogHeader>

          <div className="flex items-center justify-between">
            <label className="flex items-center gap-2 text-sm">
              <Checkbox checked={allSelected} onCheckedChange={toggleAll} />
              Select all
            </label>
            <Button size="sm" variant="outline" onClick={handleImportClick}>
              <Upload /> Import…
            </Button>
          </div>

          <ScrollArea className="h-72 rounded-md border border-border">
            <div className="flex flex-col">
              {connections.length === 0 && (
                <p className="p-3 text-sm text-muted-foreground">No connections yet.</p>
              )}
              {connections.map((connection) => (
                <label
                  key={connection.id}
                  className="flex items-center gap-2 border-b border-border px-3 py-2 text-sm last:border-b-0 hover:bg-muted"
                >
                  <Checkbox
                    checked={selectedIds.has(connection.id)}
                    onCheckedChange={(checked) => toggleSelected(connection.id, checked)}
                  />
                  <div className="min-w-0 flex-1">
                    <div className="truncate font-medium">{connection.name}</div>
                    <div className="truncate text-xs text-muted-foreground">
                      {connection.username}@{connection.host}:{connection.port}
                    </div>
                  </div>
                </label>
              ))}
            </div>
          </ScrollArea>

          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
              Close
            </Button>
            <Button
              type="button"
              variant="outline"
              disabled={selectedIds.size === 0}
              onClick={() => setExportScope({ ids: Array.from(selectedIds), count: selectedIds.size })}
            >
              <Download /> Export selected ({selectedIds.size})
            </Button>
            <Button
              type="button"
              disabled={connections.length === 0}
              onClick={() => setExportScope({ ids: null, count: connections.length })}
            >
              <Download /> Export all ({connections.length})
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {exportScope && (
        <ExportConnectionsDialog
          open={exportScope !== null}
          onOpenChange={(next) => {
            if (!next) setExportScope(null);
          }}
          ids={exportScope.ids}
          count={exportScope.count}
        />
      )}
    </>
  );
}
