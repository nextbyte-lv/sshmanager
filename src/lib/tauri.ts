import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  ConnectionInput,
  ConnectionProfile,
  SecretKind,
  TerminalEvent,
} from "@/types/connection";
import type { SftpEntry } from "@/types/sftp";

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

export function sftpDownload(sessionId: string, remotePath: string, localPath: string) {
  return invoke<void>("sftp_download", { sessionId, remotePath, localPath });
}

export function sftpUpload(sessionId: string, localPath: string, remotePath: string) {
  return invoke<void>("sftp_upload", { sessionId, localPath, remotePath });
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
