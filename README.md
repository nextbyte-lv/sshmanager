# SshManager

Personal SSH/SFTP session manager for Windows. Tauri (Rust) backend, React +
TypeScript + Vite frontend, xterm.js terminal, russh for SSH.

This is a personal tool — no installer polish, licensing, telemetry, or
auto-update, and no multi-user/cloud features are planned.

## Status

Built so far: connection list + editor (password or private-key auth),
tabs each holding their own resizable split-pane grid of live terminals,
drag-to-rearrange panes, and a per-pane SFTP browser panel that shares the
same SSH connection as its terminal (no second login). Packaged into a
standalone Windows installer (see below). Still ahead: auto-reconnect with
backoff. See `tasks/todo.md` for the full phase-by-phase build log and
`tasks/lessons.md` for gotchas already hit and fixed — read that before
touching `russh`, `react-mosaic-component`, or Tauri's window/drag-and-drop
config.

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

This produces:
- `src-tauri/target/release/sshmanager.exe` — standalone exe, just run it
- `src-tauri/target/release/bundle/msi/SshManager_<version>_x64_en-US.msi` — MSI installer
- `src-tauri/target/release/bundle/nsis/SshManager_<version>_x64-setup.exe` — NSIS installer

No signing/notarization — Windows SmartScreen will warn on first run of the
installers; that's expected for an unsigned personal build.

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

- `src/` — React/TypeScript frontend
  - `components/` — `ConnectionList`, `ConnectionEditorDialog`, `Workspace` (tab bar + per-tab pane grid), `PaneLeaf` (terminal + toggleable SFTP panel + toolbar), `TerminalPane`, `SftpPanel`
  - `lib/workspace.ts` — pure tab/pane tree helpers (create/split/remove) for the `react-mosaic-component` layout
  - `lib/tauri.ts` — typed wrappers around every Tauri command
  - `types/` — shared frontend types mirroring the Rust DTOs
- `src-tauri/src/` — Rust backend
  - `storage/` — connection metadata JSON store
  - `secrets/` — Windows Credential Manager wrapper
  - `ssh/` — russh client/auth, PTY session handling, SFTP (shares the terminal's connection)
  - `commands/` — Tauri commands exposed to the frontend
  - `state.rs` — in-memory session registry (SSH handles + SFTP session cache)

See `CLAUDE.md` for the deeper architecture notes (session-sharing model,
why tabs are hidden-not-unmounted, Windows-specific build traps).
