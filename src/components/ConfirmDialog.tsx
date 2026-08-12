import type { ReactNode } from "react";
import { useState } from "react";

import { Button } from "@/components/ui/button";
import {
    Dialog,
    DialogContent,
    DialogDescription,
    DialogFooter,
    DialogHeader,
    DialogTitle,
} from "@/components/ui/dialog";

interface ConfirmDialogProps {
    open: boolean;
    onOpenChange: (open: boolean) => void;
    title: string;
    description?: ReactNode;
    confirmLabel?: string;
    /** Shown on the confirm button while `onConfirm` is still running. */
    pendingLabel?: string;
    destructive?: boolean;
    /**
     * Awaited before the dialog closes, so the confirm button can stay disabled
     * for the whole operation. Callers surface their own failures (the SFTP
     * panel, for one, has an error banner) — this only closes the dialog so
     * whatever the caller reports is visible behind it.
     */
    onConfirm: () => Promise<void> | void;
}

// An in-app replacement for `window.confirm`: it can name the exact target and
// spell out consequences, which matters for actions that recurse into a whole
// directory tree and cannot be undone.
export function ConfirmDialog({
    open,
    onOpenChange,
    title,
    description,
    confirmLabel = "Confirm",
    pendingLabel,
    destructive = false,
    onConfirm,
}: ConfirmDialogProps) {
    const [busy, setBusy] = useState(false);

    async function handleConfirm() {
        setBusy(true);
        try {
            await onConfirm();
        } finally {
            setBusy(false);
            onOpenChange(false);
        }
    }

    return (
        <Dialog
            open={open}
            onOpenChange={(next) => {
                // Closing mid-action would leave the button state stranded and
                // hide an operation that is still running on the server.
                if (busy) return;
                onOpenChange(next);
            }}
        >
            <DialogContent className="sm:max-w-md">
                <DialogHeader>
                    <DialogTitle>{title}</DialogTitle>
                    {description && (
                        <DialogDescription>{description}</DialogDescription>
                    )}
                </DialogHeader>

                <DialogFooter>
                    <Button
                        type="button"
                        variant="outline"
                        onClick={() => onOpenChange(false)}
                        disabled={busy}
                    >
                        Cancel
                    </Button>
                    <Button
                        type="button"
                        variant={destructive ? "destructive" : "default"}
                        onClick={handleConfirm}
                        disabled={busy}
                    >
                        {busy ? (pendingLabel ?? confirmLabel) : confirmLabel}
                    </Button>
                </DialogFooter>
            </DialogContent>
        </Dialog>
    );
}
