export interface SftpEntry {
  name: string;
  is_dir: boolean;
  size: number | null;
  modified: number | null;
}

export type UploadEvent =
  | { type: "started"; path: string; total_bytes: number }
  | { type: "progress"; path: string; bytes_done: number; total_bytes: number }
  | { type: "skipped"; path: string }
  | { type: "file_done"; path: string }
  | { type: "file_error"; path: string; message: string }
  | { type: "done"; uploaded: number; skipped: number; failed: number };
