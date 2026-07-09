# SshManager

Personal SSH/SFTP session manager for Windows. Tauri (Rust) backend, React +
TypeScript + Vite frontend, xterm.js terminal, russh for SSH.

This is a personal tool — no installer polish, licensing, telemetry, or
auto-update, and no multi-user/cloud features are planned.

## Status

**Phase 1 (this build):** connection list + editor, password or private-key
auth, one live xterm.js terminal per connection. See `tasks/todo.md` for what's
done and what's still ahead — split-pane tiling, tabs, an SFTP browser panel,
auto-reconnect, and packaging into a standalone installer are deferred to later
phases.

## Running it

Prerequisites: Node.js, Rust (stable, MSVC toolchain), and the Tauri CLI is
pulled in as a dev dependency — no global install needed.

```
npm install
npm run tauri dev
```

This starts the Vite dev server and launches the desktop app with hot reload
for the frontend (Rust changes trigger a rebuild + relaunch).

To produce a standalone Windows build:

```
npm run tauri build
```

The `.exe`/installer land under `src-tauri/target/release/bundle/`.

## Where things are stored

- **Connection metadata** (host, port, username, auth type, tags) — a plain
  JSON file at `%APPDATA%\com.arccuks.sshmanager\connections.json`. No secrets
  are ever written here.
- **Passwords and key passphrases** — Windows Credential Manager (DPAPI-backed,
  tied to your Windows account), via the `keyring` crate. Each secret is stored
  under a target name like `sshmanager:<connection-id>:password` (or
  `:passphrase` for key auth), scoped to the connection's username. You can see
  these directly in Credential Manager, or via `cmdkey /list` in a terminal.
- **Private key files** stay wherever you pointed the connection editor at them
  on disk — SshManager only stores the path, never the key material.

There is no master password and no custom encryption layer; secrecy relies
entirely on Windows' own account-bound credential store.

## Project layout

- `src/` — React/TypeScript frontend (components, xterm.js wiring, Tauri IPC calls)
- `src-tauri/src/` — Rust backend
  - `storage/` — connection metadata JSON store
  - `secrets/` — Windows Credential Manager wrapper
  - `ssh/` — russh client/auth and PTY session handling
  - `commands/` — Tauri commands exposed to the frontend
