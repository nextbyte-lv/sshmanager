import { Fragment, useEffect, useState } from "react";

import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
    Dialog,
    DialogContent,
    DialogDescription,
    DialogFooter,
    DialogHeader,
    DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
    formatOctal,
    formatSymbolic,
    hasBit,
    hasSpecial,
    MODE_PRESETS,
    parseOctal,
    SET_GID,
    SET_UID,
    STICKY,
    withBit,
    withSpecial,
    type What,
    type Who,
} from "@/lib/permissions";
import type { SftpEntry } from "@/types/sftp";

const WHO_ROWS: { who: Who; label: string }[] = [
    { who: "owner", label: "Owner" },
    { who: "group", label: "Group" },
    { who: "other", label: "Others" },
];

const WHAT_COLUMNS: { what: What; label: string }[] = [
    { what: "read", label: "Read" },
    { what: "write", label: "Write" },
    { what: "exec", label: "Execute" },
];

const SPECIAL_BITS: { bit: number; label: string; title: string }[] = [
    { bit: SET_UID, label: "setuid", title: "Runs as the file's owner rather than whoever started it" },
    {
        bit: SET_GID,
        label: "setgid",
        title: "Runs as the file's group; on a folder, new entries inherit that group",
    },
    { bit: STICKY, label: "sticky", title: "On a folder, only an entry's owner may delete it" },
];

interface PermissionsDialogProps {
    open: boolean;
    onOpenChange: (open: boolean) => void;
    /** The entry being edited — null whenever the dialog is closed. */
    entry: SftpEntry | null;
    /** Absolute remote path of `entry`, shown so there's no doubt what is changing. */
    path: string;
    /**
     * Applies the mode. Rejections are caught and shown in the dialog, which stays
     * open so the value can be corrected — a refused chmod usually means the mode
     * was fine and the *file* was the problem, and the message says which.
     */
    onApply: (mode: number, recursive: boolean) => Promise<void>;
}

// Editing a remote mode by hand means either remembering what 2775 does or
// reasoning it out from `ls -l`. This shows both at once: the octal field, the
// checkbox grid and the symbolic string are all the same number.
export function PermissionsDialog({ open, onOpenChange, entry, path, onApply }: PermissionsDialogProps) {
    const [mode, setMode] = useState(0);
    // Holds what's being typed in the octal field, including values too short to
    // parse. Null means "show the mode" — which is what the grid sets it back to,
    // so the field can never sit there disagreeing with the checkboxes.
    const [octalDraft, setOctalDraft] = useState<string | null>(null);
    const [recursive, setRecursive] = useState(false);
    const [busy, setBusy] = useState(false);
    const [error, setError] = useState<string | null>(null);

    useEffect(() => {
        if (!open) return;
        setMode(entry?.mode ?? 0);
        setOctalDraft(null);
        setRecursive(false);
        setError(null);
    }, [open, entry]);

    function updateMode(next: number) {
        setOctalDraft(null);
        setMode(next);
    }

    function handleOctalChange(text: string) {
        setOctalDraft(text);
        const parsed = parseOctal(text);
        if (parsed !== null) setMode(parsed);
    }

    async function handleApply() {
        setBusy(true);
        setError(null);
        try {
            await onApply(mode, recursive);
            onOpenChange(false);
        } catch (err) {
            setError(String(err));
        } finally {
            setBusy(false);
        }
    }

    const octalText = octalDraft ?? formatOctal(mode);
    const octalInvalid = octalDraft !== null && parseOctal(octalDraft) === null;

    return (
        <Dialog
            open={open}
            onOpenChange={(next) => {
                if (busy) return;
                onOpenChange(next);
            }}
        >
            <DialogContent className="sm:max-w-md">
                <DialogHeader>
                    <DialogTitle>Permissions</DialogTitle>
                    <DialogDescription className="font-mono break-all text-foreground">
                        {path}
                    </DialogDescription>
                </DialogHeader>

                <div className="grid gap-3">
                    <div className="flex items-end gap-2">
                        <div className="grid gap-1.5">
                            <Label htmlFor="mode-octal">Octal</Label>
                            <Input
                                id="mode-octal"
                                className="w-20 font-mono"
                                inputMode="numeric"
                                autoComplete="off"
                                spellCheck={false}
                                value={octalText}
                                aria-invalid={octalInvalid}
                                onChange={(e) => handleOctalChange(e.target.value.trim())}
                                onBlur={() => setOctalDraft(null)}
                            />
                        </div>
                        <span className="pb-1.5 font-mono text-muted-foreground">
                            {formatSymbolic(mode, entry?.is_dir ?? false, entry?.is_symlink ?? false)}
                        </span>
                    </div>

                    <div className="grid grid-cols-[auto_repeat(3,minmax(0,1fr))] items-center gap-x-3 gap-y-2">
                        <span />
                        {WHAT_COLUMNS.map(({ what, label }) => (
                            <span key={what} className="text-center text-xs text-muted-foreground">
                                {label}
                            </span>
                        ))}
                        {WHO_ROWS.map(({ who, label }) => (
                            <Fragment key={who}>
                                <span className="text-xs text-muted-foreground">{label}</span>
                                {WHAT_COLUMNS.map(({ what }) => (
                                    <div key={what} className="flex justify-center">
                                        <Checkbox
                                            aria-label={`${label} ${what}`}
                                            checked={hasBit(mode, who, what)}
                                            onCheckedChange={(checked) =>
                                                updateMode(withBit(mode, who, what, checked))
                                            }
                                        />
                                    </div>
                                ))}
                            </Fragment>
                        ))}
                    </div>

                    <div className="flex flex-wrap items-center gap-3">
                        {SPECIAL_BITS.map(({ bit, label, title }) => (
                            <label
                                key={label}
                                className="flex items-center gap-1.5 text-xs text-muted-foreground"
                                title={title}
                            >
                                <Checkbox
                                    checked={hasSpecial(mode, bit)}
                                    onCheckedChange={(checked) =>
                                        updateMode(withSpecial(mode, bit, checked))
                                    }
                                />
                                <span>{label}</span>
                            </label>
                        ))}
                    </div>

                    <div className="flex flex-wrap items-center gap-1.5">
                        {MODE_PRESETS.map(({ mode: preset, hint }) => (
                            <Button
                                key={preset}
                                type="button"
                                size="xs"
                                variant={mode === preset ? "default" : "outline"}
                                className="font-mono"
                                title={hint}
                                onClick={() => updateMode(preset)}
                            >
                                {formatOctal(preset)}
                            </Button>
                        ))}
                    </div>

                    {entry?.is_dir && (
                        <label className="flex items-start gap-2 text-sm">
                            <Checkbox checked={recursive} onCheckedChange={setRecursive} className="mt-0.5" />
                            <span>
                                Apply to everything inside this folder
                                <span className="block text-xs text-muted-foreground">
                                    The same mode for every file and subfolder, as{" "}
                                    <span className="font-mono">chmod -R</span> would. Symlinks are left
                                    alone.
                                </span>
                            </span>
                        </label>
                    )}

                    {entry?.is_symlink && (
                        <p className="text-xs text-muted-foreground">
                            This is a symlink, and a mode change follows it — the file it points at is
                            what changes, not the link.
                        </p>
                    )}

                    {entry !== null && (entry.uid !== null || entry.gid !== null) && (
                        <p className="text-xs text-muted-foreground">
                            Owned by uid {entry.uid ?? "?"}, gid {entry.gid ?? "?"} — a mode change on
                            something this login doesn't own is retried with sudo.
                        </p>
                    )}

                    {error && <p className="text-sm text-destructive">{error}</p>}
                </div>

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
                        onClick={handleApply}
                        disabled={busy || octalInvalid || entry === null}
                    >
                        {busy ? "Applying…" : "Apply"}
                    </Button>
                </DialogFooter>
            </DialogContent>
        </Dialog>
    );
}
