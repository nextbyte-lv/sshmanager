import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  ConnectionInput,
  ConnectionProfile,
  SecretKind,
  TerminalEvent,
} from "@/types/connection";
import type {
  DirSize,
  FileSyncEvent,
  SftpEntry,
  UploadEvent,
} from "@/types/sftp";

export function listConnections() {
  return invoke<ConnectionProfile[]>("list_connections");
}

export function saveConnection(id: string | null, input: ConnectionInput) {
  return invoke<ConnectionProfile>("save_connection", { id, input });
}

export function deleteConnection(id: string) {
  return invoke<void>("delete_connection", { id });
}

export function duplicateConnection(id: string) {
  return invoke<ConnectionProfile>("duplicate_connection", { id });
}

export function saveCredential(id: string, username: string, kind: SecretKind, secret: string) {
  return invoke<void>("save_credential", { id, username, kind, secret });
}

export function hasCredential(id: string, username: string, kind: SecretKind) {
  return invoke<boolean>("has_credential", { id, username, kind });
}

export function addFavoritePath(id: string, label: string, path: string) {
  return invoke<ConnectionProfile>("add_favorite_path", { id, label, path });
}

export function removeFavoritePath(id: string, favoriteId: string) {
  return invoke<ConnectionProfile>("remove_favorite_path", { id, favoriteId });
}

export function exportConnections(path: string, ids: string[] | null, includeSecrets: boolean) {
  return invoke<void>("export_connections", { path, ids, includeSecrets });
}

export function importConnections(path: string) {
  return invoke<ConnectionProfile[]>("import_connections", { path });
}

export function testConnection(id: string | null, input: ConnectionInput, secret: string | null) {
  return invoke<void>("test_connection", { id, input, secret });
}

export function openSession(
  id: string,
  cols: number,
  rows: number,
  onEvent: (event: TerminalEvent) => void,
) {
  const channel = new Channel<TerminalEvent>();
  channel.onmessage = onEvent;
  return invoke<string>("open_session", { id, cols, rows, onEvent: channel });
}

export function sendInput(sessionId: string, data: string) {
  return invoke<void>("send_input", { sessionId, data });
}

export function resizeSession(sessionId: string, cols: number, rows: number) {
  return invoke<void>("resize_session", { sessionId, cols, rows });
}

export function closeSession(sessionId: string) {
  return invoke<void>("close_session", { sessionId });
}

export function sftpCanonicalize(sessionId: string, path: string) {
  return invoke<string>("sftp_canonicalize", { sessionId, path });
}

export function sftpListDir(sessionId: string, path: string) {
  return invoke<SftpEntry[]>("sftp_list_dir", { sessionId, path });
}

export function sftpDirSizes(sessionId: string, paths: string[]) {
  return invoke<DirSize[]>("sftp_dir_sizes", { sessionId, paths });
}

export function sftpDownload(sessionId: string, remotePath: string, localPath: string) {
  return invoke<void>("sftp_download", { sessionId, remotePath, localPath });
}

export function sftpUpload(
  sessionId: string,
  localPath: string,
  remotePath: string,
  onEvent: (event: UploadEvent) => void,
) {
  const channel = new Channel<UploadEvent>();
  channel.onmessage = onEvent;
  return invoke<void>("sftp_upload", { sessionId, localPath, remotePath, onEvent: channel });
}

export function sftpMkdir(sessionId: string, path: string) {
  return invoke<void>("sftp_mkdir", { sessionId, path });
}

export function sftpDelete(sessionId: string, path: string, isDir: boolean) {
  return invoke<void>("sftp_delete", { sessionId, path, isDir });
}

export function sftpRename(sessionId: string, from: string, to: string) {
  return invoke<void>("sftp_rename", { sessionId, from, to });
}

export function sftpSetMode(sessionId: string, path: string, mode: number, recursive: boolean) {
  return invoke<void>("sftp_set_mode", { sessionId, path, mode, recursive });
}

export function sftpOpenFile(
  sessionId: string,
  remotePath: string,
  onEvent: (event: FileSyncEvent) => void,
) {
  const channel = new Channel<FileSyncEvent>();
  channel.onmessage = onEvent;
  return invoke<string>("sftp_open_file", { sessionId, remotePath, onEvent: channel });
}
