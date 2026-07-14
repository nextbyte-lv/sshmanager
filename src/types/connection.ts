export type AuthType = "password" | "key";

export interface FavoritePath {
  id: string;
  label: string;
  path: string;
}

export interface ConnectionProfile {
  id: string;
  name: string;
  host: string;
  port: number;
  username: string;
  auth_type: AuthType;
  key_path?: string | null;
  tags: string[];
  last_used_at?: number | null;
  color?: string | null;
  favorite_paths: FavoritePath[];
}

export interface ConnectionInput {
  name: string;
  host: string;
  port: number;
  username: string;
  auth_type: AuthType;
  key_path?: string | null;
  tags: string[];
  color?: string | null;
}

export type SecretKind = "password" | "passphrase";

export type TerminalEvent =
  | { type: "data"; data: string }
  | { type: "closed"; code: number | null }
  | { type: "error"; message: string }
  | { type: "reconnecting"; attempt: number; max_attempts: number }
  | { type: "reconnected" };
