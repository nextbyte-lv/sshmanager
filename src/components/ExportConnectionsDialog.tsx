import { useState } from "react";
import { save as saveFileDialog } from "@tauri-apps/plugin-dialog";

import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { exportConnections } from "@/lib/tauri";

interface ExportConnectionsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  ids: string[] | null;
  count: number;
}

export function ExportConnectionsDialog({ open, onOpenChange, ids, count }: ExportConnectionsDialogProps) {
  const [includeSecrets, setIncludeSecrets] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const scopeLabel =
    ids === null
      ? `Export all ${count} connection${count === 1 ? "" : "s"}`
      : `Export ${count} selected connection${count === 1 ? "" : "s"}`;

  async function handleExport() {
    setExporting(true);
    setError(null);
    try {
      const path = await saveFileDialog({
        defaultPath: "connections-export.json",
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!path) return;
      await exportConnections(path, ids, includeSecrets);
      onOpenChange(false);
    } catch (err) {
      setError(String(err));
    } finally {
      setExporting(false);
    }
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) setIncludeSecrets(false);
        onOpenChange(next);
      }}
    >
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{scopeLabel}</DialogTitle>
        </DialogHeader>

        <div className="grid gap-3">
          <label className="flex items-start gap-2 text-sm">
            <Checkbox checked={includeSecrets} onCheckedChange={setIncludeSecrets} className="mt-0.5" />
            <span>Include saved passwords/passphrases (plaintext)</span>
          </label>

          {includeSecrets && (
            <p className="text-sm text-destructive">
              The exported file will contain plaintext credentials — store and share it carefully.
            </p>
          )}

          {error && <p className="text-sm text-destructive">{error}</p>}
        </div>

        <DialogFooter>
          <Button type="button" variant="outline" onClick={() => onOpenChange(false)} disabled={exporting}>
            Cancel
          </Button>
          <Button type="button" onClick={handleExport} disabled={exporting}>
            {exporting ? "Exporting…" : "Export"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
