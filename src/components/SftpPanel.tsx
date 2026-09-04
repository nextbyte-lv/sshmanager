import {
    open as openFileDialog,
    save as saveFileDialog,
} from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";
import {
    ArrowUp,
    ExternalLink,
    File,
    Folder,
    FolderPlus,
    FolderUp,
    HardDrive,
    Loader2,
    Lock,
    Pencil,
    RefreshCw,
    Sigma,
    Star,
    Terminal,
    Trash2,
    Upload,
    X,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";

import { ConfirmDialog } from "@/components/ConfirmDialog";
import { PermissionsDialog } from "@/components/PermissionsDialog";
import { Button } from "@/components/ui/button";
import {
    DropdownMenu,
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuSeparator,
    DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { formatOctal, formatSymbolic } from "@/lib/permissions";
import {
    addFavoritePath,
    removeFavoritePath,
    sendInput,
    sftpCanonicalize,
    sftpDelete,
    sftpDirSizes,
    sftpDownload,
    sftpListDir,
    sftpMkdir,
    sftpOpenFile,
    sftpRename,
    sftpSetMode,
    sftpUpload,
} from "@/lib/tauri";
import type { ConnectionProfile } from "@/types/connection";
import type { DirSize, FileSyncEvent, SftpEntry, UploadEvent } from "@/types/sftp";

interface UploadProgress {
    currentPath: string;
    bytesDone: number;
    totalBytes: number;
    uploaded: number;
    skipped: number;
    failed: number;
    speedBps: number | null;
    etaSeconds: number | null;
}

interface SftpPanelProps {
    sessionId: string;
    width: number;
    connection: ConnectionProfile;
    onConnectionsChanged: () => void;
}

// Paths are always absolute (resolved once via sftp_canonicalize on mount) so
// navigation isn't artificially capped at the login directory.
function joinPath(dir: string, name: string): string {
    return dir === "/" ? `/${name}` : `${dir}/${name}`;
}

function parentPath(dir: string): string {
    if (dir === "/") return "/";
    const idx = dir.lastIndexOf("/");
    return idx <= 0 ? "/" : dir.slice(0, idx);
}

function shellQuote(path: string): string {
    return `'${path.replace(/'/g, "'\\''")}'`;
}

function formatSize(size: number | null): string {
    if (size === null) return "";
    if (size < 1024) return `${size} B`;
    if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`;
    if (size < 1024 * 1024 * 1024)
        return `${(size / (1024 * 1024)).toFixed(1)} MB`;
    return `${(size / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}

function formatModified(modified: number | null): string {
    if (modified === null) return "";
    const d = new Date(modified * 1000);
    const p = (n: number) => String(n).padStart(2, "0");
    return (
        `${p(d.getDate())}.${p(d.getMonth() + 1)}.${d.getFullYear()}` +
        ` ${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`
    );
}

function formatSpeed(bps: number | null): string {
    if (bps === null || bps <= 0) return "";
    if (bps < 1024) return `${bps.toFixed(0)} B/s`;
    if (bps < 1024 * 1024) return `${(bps / 1024).toFixed(1)} KB/s`;
    if (bps < 1024 * 1024 * 1024)
        return `${(bps / (1024 * 1024)).toFixed(1)} MB/s`;
    return `${(bps / (1024 * 1024 * 1024)).toFixed(1)} GB/s`;
}

function formatEta(seconds: number | null): string {
    if (seconds === null || !Number.isFinite(seconds) || seconds < 0)
        return "";
    const total = Math.round(seconds);
    if (total < 60) return `${total}s left`;
    const minutes = Math.floor(total / 60);
    const secs = total % 60;
    if (minutes < 60) return `${minutes}m ${secs}s left`;
    const hours = Math.floor(minutes / 60);
    const mins = minutes % 60;
    return `${hours}h ${mins}m left`;
}

// The size column. A file size comes straight from the listing; a folder has no
// size in SFTP at all, so it stays a button until someone asks for it to be
// measured.
function EntrySize({
    entry,
    size,
    sizing,
    onCalculate,
}: {
    entry: SftpEntry;
    size: DirSize | undefined;
    sizing: boolean;
    onCalculate: () => void;
}) {
    if (!entry.is_dir)
        return (
            <span className="shrink-0 text-xs text-muted-foreground">
                {formatSize(entry.size)}
            </span>
        );
    if (sizing)
        return (
            <Loader2 className="size-3.5 shrink-0 animate-spin text-muted-foreground" />
        );
    if (size)
        return (
            <span
                className="shrink-0 text-xs text-muted-foreground"
                title={
                    size.partial
                        ? `at least ${formatSize(size.bytes)} — part of this folder could not be read`
                        : undefined
                }
            >
                {size.partial ? "~" : ""}
                {formatSize(size.bytes)}
            </span>
        );
    return (
        <button
            type="button"
            className="shrink-0 cursor-pointer text-xs text-muted-foreground hover:text-foreground"
            title="Calculate folder size"
            onClick={onCalculate}
        >
            –
        </button>
    );
}

export function SftpPanel({
    sessionId,
    width,
    connection,
    onConnectionsChanged,
}: SftpPanelProps) {
    const [path, setPath] = useState<string | null>(null);
    const [entries, setEntries] = useState<SftpEntry[]>([]);
    const [loading, setLoading] = useState(false);
    // Two error slots, deliberately separate. `listError` belongs to the current
    // listing and every refresh clears it; `actionError` belongs to the last thing
    // the user asked for, and only their next action clears it. They shared one
    // slot before, and the `refresh()` at the end of an upload wiped the very
    // message the upload had just produced — a transfer that failed outright
    // looked like the button doing nothing at all.
    const [listError, setListError] = useState<string | null>(null);
    const [actionError, setActionError] = useState<string | null>(null);
    const [uploadProgress, setUploadProgress] = useState<UploadProgress | null>(
        null,
    );
    const [syncStatus, setSyncStatus] = useState<Record<string, FileSyncEvent>>(
        {},
    );
    // Folder sizes are keyed by absolute path and only ever populated on request:
    // each one costs a full walk of the tree on the remote, so nothing here is
    // computed just because a directory was listed.
    const [dirSizes, setDirSizes] = useState<Record<string, DirSize>>({});
    const [sizingPaths, setSizingPaths] = useState<string[]>([]);
    const [pendingDelete, setPendingDelete] = useState<SftpEntry | null>(null);
    const [permissionsTarget, setPermissionsTarget] = useState<SftpEntry | null>(
        null,
    );
    const uploadSampleRef = useRef<{ time: number; bytes: number } | null>(
        null,
    );
    // File-level failures reported over the upload channel, collected so the
    // summary shown once the transfer ends can count and name them.
    const uploadErrorsRef = useRef<string[]>([]);

    useEffect(() => {
        sftpCanonicalize(sessionId, ".")
            .then(setPath)
            .catch((err) => setListError(String(err)));
    }, [sessionId]);

    const refresh = useCallback(() => {
        if (path === null) return;
        setLoading(true);
        setListError(null);
        // A size measured against the previous listing must not outlive it.
        setDirSizes({});
        sftpListDir(sessionId, path)
            .then(setEntries)
            .catch((err) => setListError(String(err)))
            .finally(() => setLoading(false));
    }, [sessionId, path]);

    useEffect(() => {
        refresh();
    }, [refresh]);

    // A failure belongs to the directory it happened in: leaving it on screen
    // while the user browses elsewhere would pin it to the wrong listing.
    useEffect(() => {
        setActionError(null);
    }, [path]);

    function handleCdHere() {
        if (path === null) return;
        void sendInput(sessionId, `cd ${shellQuote(path)}\n`);
    }

    function handleUploadEvent(event: UploadEvent) {
        if (event.type === "file_error") {
            const message = `${event.path}: ${event.message}`;
            uploadErrorsRef.current.push(message);
            setActionError(message);
        }
        const now = Date.now();
        setUploadProgress((prev) => {
            if (!prev) return prev;
            switch (event.type) {
                case "started":
                    uploadSampleRef.current = { time: now, bytes: 0 };
                    return {
                        ...prev,
                        currentPath: event.path,
                        bytesDone: 0,
                        totalBytes: event.total_bytes,
                        speedBps: null,
                        etaSeconds: null,
                    };
                case "progress": {
                    const sample = uploadSampleRef.current;
                    let speedBps = prev.speedBps;
                    if (sample) {
                        const dtSeconds = (now - sample.time) / 1000;
                        const dBytes = event.bytes_done - sample.bytes;
                        // Throttle sampling so speed isn't recalculated from
                        // near-zero time deltas between rapid chunk events.
                        if (dtSeconds > 0.15 && dBytes >= 0) {
                            const instantSpeed = dBytes / dtSeconds;
                            speedBps =
                                speedBps === null
                                    ? instantSpeed
                                    : speedBps * 0.7 + instantSpeed * 0.3;
                            uploadSampleRef.current = {
                                time: now,
                                bytes: event.bytes_done,
                            };
                        }
                    }
                    const remaining = event.total_bytes - event.bytes_done;
                    const etaSeconds =
                        speedBps && speedBps > 0
                            ? remaining / speedBps
                            : null;
                    return {
                        ...prev,
                        currentPath: event.path,
                        bytesDone: event.bytes_done,
                        totalBytes: event.total_bytes,
                        speedBps,
                        etaSeconds,
                    };
                }
                case "skipped":
                    return { ...prev, skipped: prev.skipped + 1 };
                case "file_done":
                    return { ...prev, uploaded: prev.uploaded + 1 };
                case "file_error":
                    return { ...prev, failed: prev.failed + 1 };
                case "done":
                    return prev;
            }
        });
    }

    async function uploadPaths(localPaths: string[]) {
        if (path === null || localPaths.length === 0) return;
        uploadSampleRef.current = null;
        uploadErrorsRef.current = [];
        setActionError(null);
        setUploadProgress({
            currentPath: "",
            bytesDone: 0,
            totalBytes: 0,
            uploaded: 0,
            skipped: 0,
            failed: 0,
            speedBps: null,
            etaSeconds: null,
        });
        for (const localPath of localPaths) {
            const fileName = localPath.split(/[/\\]/).pop() ?? localPath;
            try {
                await sftpUpload(
                    sessionId,
                    localPath,
                    joinPath(path, fileName),
                    handleUploadEvent,
                );
            } catch (err) {
                // The command itself rejected, so no `file_error` event ever
                // arrived for this path — nothing else would mention it.
                uploadErrorsRef.current.push(`${fileName}: ${String(err)}`);
            }
        }
        setUploadProgress(null);
        refresh();
        // After the refresh on purpose: `refresh()` only clears its own error, and
        // this summary is the last word on the transfer either way.
        const failures = uploadErrorsRef.current;
        if (failures.length === 1) {
            setActionError(`Upload failed — ${failures[0]}`);
        } else if (failures.length > 1) {
            setActionError(
                `${failures.length} uploads failed — ${failures[0]} (and ${failures.length - 1} more)`,
            );
        }
    }

    // Files and folders differ only in the dialog flag, so both toolbar buttons
    // land here. The picker call sits inside the try because a dialog that fails
    // to open rejects, and an unhandled rejection in a click handler is precisely
    // the silent "nothing happened" this panel must never show.
    async function pickAndUpload(directory: boolean) {
        if (path === null) return;
        let localPaths: string[];
        try {
            const selected = await openFileDialog({
                directory,
                multiple: true,
                title: (directory ? "Upload folder to " : "Upload to ") + path,
            });
            localPaths = Array.isArray(selected)
                ? selected
                : selected
                  ? [selected]
                  : [];
        } catch (err) {
            setActionError(`Could not open the file picker: ${String(err)}`);
            return;
        }
        await uploadPaths(localPaths);
    }

    async function handleDownload(entry: SftpEntry) {
        if (path === null) return;
        let savePath: string | null;
        try {
            savePath = await saveFileDialog({ defaultPath: entry.name });
        } catch (err) {
            setActionError(`Could not open the save dialog: ${String(err)}`);
            return;
        }
        if (!savePath) return;
        try {
            await sftpDownload(sessionId, joinPath(path, entry.name), savePath);
        } catch (err) {
            setActionError(String(err));
        }
    }

    async function handleOpen(entry: SftpEntry) {
        if (path === null) return;
        const remotePath = joinPath(path, entry.name);
        try {
            const localPath = await sftpOpenFile(
                sessionId,
                remotePath,
                (event) => {
                    setSyncStatus((prev) => ({
                        ...prev,
                        [remotePath]: event,
                    }));
                    if (event.type === "error") setActionError(event.message);
                },
            );
            await openPath(localPath);
        } catch (err) {
            setActionError(String(err));
        }
    }

    // One request for however many folders were asked for: the backend turns the
    // whole list into a single `du`, so sizing a directory full of folders costs
    // the same round trip as sizing one of them.
    async function calculateDirSizes(paths: string[]) {
        if (paths.length === 0) return;
        setSizingPaths((prev) =>
            prev.concat(paths.filter((p) => !prev.includes(p))),
        );
        try {
            const sizes = await sftpDirSizes(sessionId, paths);
            setDirSizes((prev) => {
                const next = { ...prev };
                for (const size of sizes) next[size.path] = size;
                return next;
            });
        } catch (err) {
            setActionError(String(err));
        } finally {
            setSizingPaths((prev) => prev.filter((p) => !paths.includes(p)));
        }
    }

    async function handleNewFolder() {
        if (path === null) return;
        const name = window.prompt("New folder name");
        if (!name) return;
        try {
            await sftpMkdir(sessionId, joinPath(path, name));
            refresh();
        } catch (err) {
            setActionError(String(err));
        }
    }

    async function handleRename(entry: SftpEntry) {
        if (path === null) return;
        const name = window.prompt("Rename to", entry.name);
        if (!name || name === entry.name) return;
        try {
            await sftpRename(
                sessionId,
                joinPath(path, entry.name),
                joinPath(path, name),
            );
            refresh();
        } catch (err) {
            setActionError(String(err));
        }
    }

    async function handleDelete(entry: SftpEntry) {
        if (path === null) return;
        try {
            await sftpDelete(
                sessionId,
                joinPath(path, entry.name),
                entry.is_dir,
            );
            refresh();
        } catch (err) {
            setActionError(String(err));
        }
    }

    // Left to throw: the dialog shows the failure itself and stays open, so a
    // refusal the sudo retry couldn't get past can be read next to the mode that
    // caused it.
    async function handleSetMode(
        entry: SftpEntry,
        mode: number,
        recursive: boolean,
    ) {
        if (path === null) return;
        await sftpSetMode(sessionId, joinPath(path, entry.name), mode, recursive);
        refresh();
    }

    async function handleAddFavorite() {
        if (path === null) return;
        const defaultLabel =
            path === "/" ? "/" : (path.split("/").pop() ?? path);
        const label = window.prompt("Favorite name", defaultLabel);
        if (!label) return;
        try {
            await addFavoritePath(connection.id, label, path);
            onConnectionsChanged();
        } catch (err) {
            setActionError(String(err));
        }
    }

    async function handleRemoveFavorite(favoriteId: string) {
        try {
            await removeFavoritePath(connection.id, favoriteId);
            onConnectionsChanged();
        } catch (err) {
            setActionError(String(err));
        }
    }

    // Folders in this listing with no size yet: what the toolbar button measures,
    // and what tells it whether there is anything left to measure.
    const unsizedFolders =
        path === null
            ? []
            : entries
                  .filter(
                      (entry) =>
                          entry.is_dir &&
                          entry.name !== "." &&
                          entry.name !== "..",
                  )
                  .map((entry) => joinPath(path, entry.name))
                  .filter(
                      (folder) =>
                          !(folder in dirSizes) &&
                          !sizingPaths.includes(folder),
                  );

    const segments =
        path === null || path === "/" ? [] : path.split("/").filter(Boolean);

    return (
        <div
            className="flex h-full shrink-0 flex-col border-l border-border bg-card"
            style={{ width }}
        >
            <div className="flex items-center gap-1 border-b border-border p-1.5">
                <Button
                    size="icon-xs"
                    variant="ghost"
                    title="Up one level"
                    onClick={() => path !== null && setPath(parentPath(path))}
                >
                    <ArrowUp />
                </Button>
                <Button
                    size="icon-xs"
                    variant="ghost"
                    title="Refresh"
                    onClick={refresh}
                >
                    <RefreshCw />
                </Button>
                <Button
                    size="icon-xs"
                    variant="ghost"
                    title="Calculate folder sizes"
                    disabled={unsizedFolders.length === 0}
                    onClick={() => void calculateDirSizes(unsizedFolders)}
                >
                    <Sigma />
                </Button>
                <Button
                    size="icon-xs"
                    variant="ghost"
                    title="New folder"
                    onClick={handleNewFolder}
                >
                    <FolderPlus />
                </Button>
                <Button
                    size="icon-xs"
                    variant="ghost"
                    title="Upload files"
                    disabled={uploadProgress !== null}
                    onClick={() => void pickAndUpload(false)}
                >
                    <Upload />
                </Button>
                <Button
                    size="icon-xs"
                    variant="ghost"
                    title="Upload folder"
                    disabled={uploadProgress !== null}
                    onClick={() => void pickAndUpload(true)}
                >
                    <FolderUp />
                </Button>
                <Button
                    size="icon-xs"
                    variant="ghost"
                    title="cd here in terminal"
                    onClick={handleCdHere}
                >
                    <Terminal />
                </Button>
                <DropdownMenu>
                    <DropdownMenuTrigger
                        render={
                            <Button
                                size="icon-xs"
                                variant="ghost"
                                title="Favorites"
                            />
                        }
                    >
                        <Star />
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="start">
                        <DropdownMenuItem
                            disabled={path === null}
                            onClick={handleAddFavorite}
                        >
                            Add to favorites
                        </DropdownMenuItem>
                        <DropdownMenuSeparator />
                        {connection.favorite_paths.length === 0 && (
                            <div className="px-2 py-1.5 text-xs text-muted-foreground">
                                No favorites yet
                            </div>
                        )}
                        {connection.favorite_paths.map((fav) => (
                            <DropdownMenuItem
                                key={fav.id}
                                className="flex items-center justify-between gap-2"
                                onClick={() => setPath(fav.path)}
                            >
                                <span
                                    className="min-w-0 flex-1 truncate"
                                    title={fav.path}
                                >
                                    {fav.label}
                                </span>
                                <Button
                                    size="icon-xs"
                                    variant="ghost"
                                    title="Remove favorite"
                                    onClick={(e) => {
                                        e.stopPropagation();
                                        void handleRemoveFavorite(fav.id);
                                    }}
                                >
                                    <Trash2 />
                                </Button>
                            </DropdownMenuItem>
                        ))}
                    </DropdownMenuContent>
                </DropdownMenu>
            </div>

            <div className="flex items-center gap-0.5 overflow-x-auto border-b border-border px-2 py-1 text-xs">
                <button
                    className="cursor-pointer text-muted-foreground hover:text-foreground"
                    onClick={() => setPath("/")}
                    title="/"
                >
                    <HardDrive className="size-3.5" />
                </button>
                {segments.map((segment, i) => (
                    <span key={i} className="flex items-center gap-0.5">
                        <span className="text-muted-foreground">/</span>
                        <button
                            className="cursor-pointer text-muted-foreground hover:text-foreground"
                            onClick={() =>
                                setPath(
                                    "/" + segments.slice(0, i + 1).join("/"),
                                )
                            }
                        >
                            {segment}
                        </button>
                    </span>
                ))}
            </div>

            {uploadProgress && (
                <div className="border-b border-border px-2 py-1 text-xs text-muted-foreground">
                    <div className="flex items-center justify-between gap-2">
                        <span className="min-w-0 flex-1 truncate">
                            {uploadProgress.currentPath
                                ? uploadProgress.currentPath.split("/").pop()
                                : "Uploading…"}
                        </span>
                        <span className="shrink-0">
                            {uploadProgress.uploaded} uploaded,{" "}
                            {uploadProgress.skipped} skipped
                            {uploadProgress.failed > 0
                                ? `, ${uploadProgress.failed} failed`
                                : ""}
                        </span>
                    </div>
                    <div className="mt-1 h-1 w-full overflow-hidden rounded bg-muted">
                        <div
                            className="h-full bg-primary transition-[width]"
                            style={{
                                width:
                                    uploadProgress.totalBytes > 0
                                        ? `${Math.round((uploadProgress.bytesDone / uploadProgress.totalBytes) * 100)}%`
                                        : "0%",
                            }}
                        />
                    </div>
                    {(uploadProgress.speedBps !== null ||
                        uploadProgress.etaSeconds !== null) && (
                        <div className="mt-1 flex items-center justify-between gap-2">
                            <span>{formatSpeed(uploadProgress.speedBps)}</span>
                            <span>{formatEta(uploadProgress.etaSeconds)}</span>
                        </div>
                    )}
                </div>
            )}

            {actionError && (
                <div className="flex items-start gap-1 border-b border-border px-2 py-1 text-xs text-destructive">
                    <span className="min-w-0 flex-1 break-words">
                        {actionError}
                    </span>
                    <Button
                        size="icon-xs"
                        variant="ghost"
                        title="Dismiss"
                        onClick={() => setActionError(null)}
                    >
                        <X />
                    </Button>
                </div>
            )}

            {listError && (
                <p className="border-b border-border px-2 py-1 text-xs text-destructive">
                    {listError}
                </p>
            )}

            <div className="flex-1 overflow-y-auto">
                {(loading || path === null) && (
                    <p className="p-2 text-xs text-muted-foreground">
                        Loading…
                    </p>
                )}
                {!loading && path !== null && entries.length === 0 && (
                    <p className="p-2 text-xs text-muted-foreground">
                        Empty directory
                    </p>
                )}
                {!loading &&
                    path !== null &&
                    entries.map((entry) => {
                        const entryPath = joinPath(path, entry.name);
                        return (
                            <div
                                key={entry.name}
                                className="group flex items-center gap-1.5 px-2 py-1 hover:bg-muted"
                            >
                                {entry.is_dir ? (
                                    <Folder className="size-3.5 shrink-0 text-muted-foreground" />
                                ) : (
                                    <File className="size-3.5 shrink-0 text-muted-foreground" />
                                )}
                                <button
                                    type="button"
                                    className="min-w-0 flex-1 truncate text-left text-xs"
                                    onDoubleClick={() =>
                                        entry.is_dir
                                            ? setPath(entryPath)
                                            : void handleOpen(entry)
                                    }
                                    title={`${entry.name}${entry.modified ? ` — ${formatModified(entry.modified)}` : ""}`}
                                >
                                    {entry.name}
                                </button>
                                {!entry.is_dir &&
                                    (() => {
                                        const sync = syncStatus[entryPath];
                                        if (sync?.type === "uploading")
                                            return (
                                                <Loader2 className="size-3.5 shrink-0 animate-spin text-muted-foreground" />
                                            );
                                        if (
                                            sync?.type === "uploaded" &&
                                            sync.elevated
                                        )
                                            return (
                                                <span
                                                    className="shrink-0 text-[10px] uppercase text-muted-foreground"
                                                    title="Saved with sudo — this file is not writable by your login user"
                                                >
                                                    sudo
                                                </span>
                                            );
                                        return null;
                                    })()}
                                {entry.mode !== null && (
                                    <button
                                        type="button"
                                        className="shrink-0 cursor-pointer font-mono text-[10px] text-muted-foreground hover:text-foreground"
                                        title={`${formatSymbolic(entry.mode, entry.is_dir, entry.is_symlink)} — click to change permissions`}
                                        onClick={() => setPermissionsTarget(entry)}
                                    >
                                        {formatOctal(entry.mode)}
                                    </button>
                                )}
                                <EntrySize
                                    entry={entry}
                                    size={dirSizes[entryPath]}
                                    sizing={sizingPaths.includes(entryPath)}
                                    onCalculate={() =>
                                        void calculateDirSizes([entryPath])
                                    }
                                />
                                <div className="flex shrink-0 opacity-0 group-hover:opacity-100">
                                    {!entry.is_dir && (
                                        <Button
                                            size="icon-xs"
                                            variant="ghost"
                                            title="Open"
                                            onClick={() => handleOpen(entry)}
                                        >
                                            <ExternalLink />
                                        </Button>
                                    )}
                                    {!entry.is_dir && (
                                        <Button
                                            size="icon-xs"
                                            variant="ghost"
                                            title="Download"
                                            onClick={() => handleDownload(entry)}
                                        >
                                            <ArrowUp className="rotate-180" />
                                        </Button>
                                    )}
                                    <Button
                                        size="icon-xs"
                                        variant="ghost"
                                        title="Permissions"
                                        onClick={() => setPermissionsTarget(entry)}
                                    >
                                        <Lock />
                                    </Button>
                                    <Button
                                        size="icon-xs"
                                        variant="ghost"
                                        title="Rename"
                                        onClick={() => handleRename(entry)}
                                    >
                                        <Pencil />
                                    </Button>
                                    <Button
                                        size="icon-xs"
                                        variant="ghost"
                                        title="Delete"
                                        onClick={() => setPendingDelete(entry)}
                                    >
                                        <Trash2 />
                                    </Button>
                                </div>
                            </div>
                        );
                    })}
            </div>

            <PermissionsDialog
                open={permissionsTarget !== null}
                onOpenChange={(open) => {
                    if (!open) setPermissionsTarget(null);
                }}
                entry={permissionsTarget}
                path={joinPath(path ?? "", permissionsTarget?.name ?? "")}
                onApply={async (mode, recursive) => {
                    if (permissionsTarget)
                        await handleSetMode(permissionsTarget, mode, recursive);
                }}
            />

            <ConfirmDialog
                open={pendingDelete !== null}
                onOpenChange={(open) => {
                    if (!open) setPendingDelete(null);
                }}
                title={`Delete this ${pendingDelete?.is_dir ? "folder" : "file"}?`}
                description={
                    pendingDelete && (
                        <>
                            <span className="block font-mono break-all text-foreground">
                                {joinPath(path ?? "", pendingDelete.name)}
                            </span>
                            <span className="mt-2 block">
                                {pendingDelete.is_dir
                                    ? "Everything inside it is deleted too. This cannot be undone."
                                    : "This cannot be undone."}
                            </span>
                        </>
                    )
                }
                confirmLabel="Delete"
                pendingLabel="Deleting…"
                destructive
                onConfirm={async () => {
                    if (pendingDelete) await handleDelete(pendingDelete);
                }}
            />
        </div>
    );
}
