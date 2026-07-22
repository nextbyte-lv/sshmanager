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
    Pencil,
    RefreshCw,
    Star,
    Terminal,
    Trash2,
    Upload,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";

import { Button } from "@/components/ui/button";
import {
    DropdownMenu,
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuSeparator,
    DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
    addFavoritePath,
    removeFavoritePath,
    sendInput,
    sftpCanonicalize,
    sftpDelete,
    sftpDownload,
    sftpListDir,
    sftpMkdir,
    sftpOpenFile,
    sftpRename,
    sftpUpload,
} from "@/lib/tauri";
import type { ConnectionProfile } from "@/types/connection";
import type { FileSyncEvent, SftpEntry, UploadEvent } from "@/types/sftp";

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
    return new Date(modified * 1000).toLocaleString();
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

export function SftpPanel({
    sessionId,
    width,
    connection,
    onConnectionsChanged,
}: SftpPanelProps) {
    const [path, setPath] = useState<string | null>(null);
    const [entries, setEntries] = useState<SftpEntry[]>([]);
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [uploadProgress, setUploadProgress] = useState<UploadProgress | null>(
        null,
    );
    const [syncStatus, setSyncStatus] = useState<
        Record<string, FileSyncEvent["type"]>
    >({});
    const uploadSampleRef = useRef<{ time: number; bytes: number } | null>(
        null,
    );

    useEffect(() => {
        sftpCanonicalize(sessionId, ".")
            .then(setPath)
            .catch((err) => setError(String(err)));
    }, [sessionId]);

    const refresh = useCallback(() => {
        if (path === null) return;
        setLoading(true);
        setError(null);
        sftpListDir(sessionId, path)
            .then(setEntries)
            .catch((err) => setError(String(err)))
            .finally(() => setLoading(false));
    }, [sessionId, path]);

    useEffect(() => {
        refresh();
    }, [refresh]);

    function handleCdHere() {
        if (path === null) return;
        void sendInput(sessionId, `cd ${shellQuote(path)}\n`);
    }

    function handleUploadEvent(event: UploadEvent) {
        if (event.type === "file_error") {
            setError(event.message);
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
                setError(String(err));
            }
        }
        setUploadProgress(null);
        refresh();
    }

    async function handleUpload() {
        if (path === null) return;
        const selected = await openFileDialog({
            multiple: true,
            title: "Upload to " + path,
        });
        const localPaths = Array.isArray(selected)
            ? selected
            : selected
              ? [selected]
              : [];
        await uploadPaths(localPaths);
    }

    async function handleUploadFolder() {
        if (path === null) return;
        const selected = await openFileDialog({
            directory: true,
            multiple: true,
            title: "Upload folder to " + path,
        });
        const localPaths = Array.isArray(selected)
            ? selected
            : selected
              ? [selected]
              : [];
        await uploadPaths(localPaths);
    }

    async function handleDownload(entry: SftpEntry) {
        if (path === null) return;
        const savePath = await saveFileDialog({ defaultPath: entry.name });
        if (!savePath) return;
        try {
            await sftpDownload(sessionId, joinPath(path, entry.name), savePath);
        } catch (err) {
            setError(String(err));
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
                        [remotePath]: event.type,
                    }));
                    if (event.type === "error") setError(event.message);
                },
            );
            await openPath(localPath);
        } catch (err) {
            setError(String(err));
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
            setError(String(err));
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
            setError(String(err));
        }
    }

    async function handleDelete(entry: SftpEntry) {
        if (path === null) return;
        if (!window.confirm(`Delete "${entry.name}"?`)) return;
        try {
            await sftpDelete(
                sessionId,
                joinPath(path, entry.name),
                entry.is_dir,
            );
            refresh();
        } catch (err) {
            setError(String(err));
        }
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
            setError(String(err));
        }
    }

    async function handleRemoveFavorite(favoriteId: string) {
        try {
            await removeFavoritePath(connection.id, favoriteId);
            onConnectionsChanged();
        } catch (err) {
            setError(String(err));
        }
    }

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
                    onClick={handleUpload}
                >
                    <Upload />
                </Button>
                <Button
                    size="icon-xs"
                    variant="ghost"
                    title="Upload folder"
                    disabled={uploadProgress !== null}
                    onClick={handleUploadFolder}
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

            {error && (
                <p className="border-b border-border px-2 py-1 text-xs text-destructive">
                    {error}
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
                    entries.map((entry) => (
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
                                        ? setPath(joinPath(path, entry.name))
                                        : void handleOpen(entry)
                                }
                                title={`${entry.name}${entry.modified ? ` — ${formatModified(entry.modified)}` : ""}`}
                            >
                                {entry.name}
                            </button>
                            {!entry.is_dir &&
                                syncStatus[joinPath(path, entry.name)] ===
                                    "uploading" && (
                                    <Loader2 className="size-3.5 shrink-0 animate-spin text-muted-foreground" />
                                )}
                            <span className="shrink-0 text-xs text-muted-foreground">
                                {entry.is_dir ? "" : formatSize(entry.size)}
                            </span>
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
                                    title="Rename"
                                    onClick={() => handleRename(entry)}
                                >
                                    <Pencil />
                                </Button>
                                <Button
                                    size="icon-xs"
                                    variant="ghost"
                                    title="Delete"
                                    onClick={() => handleDelete(entry)}
                                >
                                    <Trash2 />
                                </Button>
                            </div>
                        </div>
                    ))}
            </div>
        </div>
    );
}
