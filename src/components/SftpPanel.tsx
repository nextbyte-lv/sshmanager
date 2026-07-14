import { useCallback, useEffect, useState } from "react";
import { open as openFileDialog, save as saveFileDialog } from "@tauri-apps/plugin-dialog";
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
  Terminal,
  Trash2,
  Upload,
} from "lucide-react";

import { Button } from "@/components/ui/button";
import {
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
import type { FileSyncEvent, SftpEntry, UploadEvent } from "@/types/sftp";

interface UploadProgress {
  currentPath: string;
  bytesDone: number;
  totalBytes: number;
  uploaded: number;
  skipped: number;
  failed: number;
}

interface SftpPanelProps {
  sessionId: string;
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
  if (size < 1024 * 1024 * 1024) return `${(size / (1024 * 1024)).toFixed(1)} MB`;
  return `${(size / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}

function formatModified(modified: number | null): string {
  if (modified === null) return "";
  return new Date(modified * 1000).toLocaleString();
}

export function SftpPanel({ sessionId }: SftpPanelProps) {
  const [path, setPath] = useState<string | null>(null);
  const [entries, setEntries] = useState<SftpEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [uploadProgress, setUploadProgress] = useState<UploadProgress | null>(null);
  const [syncStatus, setSyncStatus] = useState<Record<string, FileSyncEvent["type"]>>({});

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
    setUploadProgress((prev) => {
      if (!prev) return prev;
      switch (event.type) {
        case "started":
          return { ...prev, currentPath: event.path, bytesDone: 0, totalBytes: event.total_bytes };
        case "progress":
          return { ...prev, currentPath: event.path, bytesDone: event.bytes_done, totalBytes: event.total_bytes };
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
    setUploadProgress({ currentPath: "", bytesDone: 0, totalBytes: 0, uploaded: 0, skipped: 0, failed: 0 });
    for (const localPath of localPaths) {
      const fileName = localPath.split(/[/\\]/).pop() ?? localPath;
      try {
        await sftpUpload(sessionId, localPath, joinPath(path, fileName), handleUploadEvent);
      } catch (err) {
        setError(String(err));
      }
    }
    setUploadProgress(null);
    refresh();
  }

  async function handleUpload() {
    if (path === null) return;
    const selected = await openFileDialog({ multiple: true, title: "Upload to " + path });
    const localPaths = Array.isArray(selected) ? selected : selected ? [selected] : [];
    await uploadPaths(localPaths);
  }

  async function handleUploadFolder() {
    if (path === null) return;
    const selected = await openFileDialog({ directory: true, multiple: true, title: "Upload folder to " + path });
    const localPaths = Array.isArray(selected) ? selected : selected ? [selected] : [];
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
      const localPath = await sftpOpenFile(sessionId, remotePath, (event) => {
        setSyncStatus((prev) => ({ ...prev, [remotePath]: event.type }));
        if (event.type === "error") setError(event.message);
      });
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
      await sftpRename(sessionId, joinPath(path, entry.name), joinPath(path, name));
      refresh();
    } catch (err) {
      setError(String(err));
    }
  }

  async function handleDelete(entry: SftpEntry) {
    if (path === null) return;
    if (!window.confirm(`Delete "${entry.name}"?`)) return;
    try {
      await sftpDelete(sessionId, joinPath(path, entry.name), entry.is_dir);
      refresh();
    } catch (err) {
      setError(String(err));
    }
  }

  const segments = path === null || path === "/" ? [] : path.split("/").filter(Boolean);

  return (
    <div className="flex h-full w-80 flex-col border-l border-border bg-card">
      <div className="flex items-center gap-1 border-b border-border p-1.5">
        <Button
          size="icon-xs"
          variant="ghost"
          title="Up one level"
          onClick={() => path !== null && setPath(parentPath(path))}
        >
          <ArrowUp />
        </Button>
        <Button size="icon-xs" variant="ghost" title="Refresh" onClick={refresh}>
          <RefreshCw />
        </Button>
        <Button size="icon-xs" variant="ghost" title="New folder" onClick={handleNewFolder}>
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
        <Button size="icon-xs" variant="ghost" title="cd here in terminal" onClick={handleCdHere}>
          <Terminal />
        </Button>
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
              onClick={() => setPath("/" + segments.slice(0, i + 1).join("/"))}
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
              {uploadProgress.currentPath ? uploadProgress.currentPath.split("/").pop() : "Uploading…"}
            </span>
            <span className="shrink-0">
              {uploadProgress.uploaded} uploaded, {uploadProgress.skipped} skipped
              {uploadProgress.failed > 0 ? `, ${uploadProgress.failed} failed` : ""}
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
        </div>
      )}

      {error && <p className="border-b border-border px-2 py-1 text-xs text-destructive">{error}</p>}

      <div className="flex-1 overflow-y-auto">
        {(loading || path === null) && <p className="p-2 text-xs text-muted-foreground">Loading…</p>}
        {!loading && path !== null && entries.length === 0 && (
          <p className="p-2 text-xs text-muted-foreground">Empty directory</p>
        )}
        {!loading &&
          path !== null &&
          entries.map((entry) => (
            <div key={entry.name} className="group flex items-center gap-1.5 px-2 py-1 hover:bg-muted">
              {entry.is_dir ? (
                <Folder className="size-3.5 shrink-0 text-muted-foreground" />
              ) : (
                <File className="size-3.5 shrink-0 text-muted-foreground" />
              )}
              <button
                type="button"
                className="min-w-0 flex-1 truncate text-left text-xs"
                onDoubleClick={() =>
                  entry.is_dir ? setPath(joinPath(path, entry.name)) : void handleOpen(entry)
                }
                title={`${entry.name}${entry.modified ? ` — ${formatModified(entry.modified)}` : ""}`}
              >
                {entry.name}
              </button>
              {!entry.is_dir && syncStatus[joinPath(path, entry.name)] === "uploading" && (
                <Loader2 className="size-3.5 shrink-0 animate-spin text-muted-foreground" />
              )}
              <span className="shrink-0 text-xs text-muted-foreground">
                {entry.is_dir ? "" : formatSize(entry.size)}
              </span>
              <div className="flex shrink-0 opacity-0 group-hover:opacity-100">
                {!entry.is_dir && (
                  <Button size="icon-xs" variant="ghost" title="Open" onClick={() => handleOpen(entry)}>
                    <ExternalLink />
                  </Button>
                )}
                {!entry.is_dir && (
                  <Button size="icon-xs" variant="ghost" title="Download" onClick={() => handleDownload(entry)}>
                    <ArrowUp className="rotate-180" />
                  </Button>
                )}
                <Button size="icon-xs" variant="ghost" title="Rename" onClick={() => handleRename(entry)}>
                  <Pencil />
                </Button>
                <Button size="icon-xs" variant="ghost" title="Delete" onClick={() => handleDelete(entry)}>
                  <Trash2 />
                </Button>
              </div>
            </div>
          ))}
      </div>
    </div>
  );
}
