export interface SftpEntry {
  name: string;
  is_dir: boolean;
  is_symlink: boolean;
  size: number | null;
  modified: number | null;
  /** Mode bits only (0o7777), no file-type bits. Null if the server omitted them. */
  mode: number | null;
  uid: number | null;
  gid: number | null;
}

/** Recursive size of one directory, summed on the remote by `du`. */
export interface DirSize {
  path: string;
  bytes: number;
  /** True when part of the tree was unreadable, so `bytes` is a lower bound. */
  partial: boolean;
}

export type UploadEvent =
  | { type: "started"; path: string; total_bytes: number }
  | { type: "progress"; path: string; bytes_done: number; total_bytes: number }
  | { type: "skipped"; path: string }
  | { type: "file_done"; path: string }
  | { type: "file_error"; path: string; message: string }
  | { type: "done"; uploaded: number; skipped: number; failed: number };

export type FileSyncEvent =
  | { type: "uploading" }
  | { type: "uploaded"; elevated: boolean }
  | { type: "error"; message: string };
