# Phase 1 (MVP) — Single-session SSH terminal

Plan: scaffold the app, wire a real SSH connection (password or key auth) into one
xterm.js pane, with a connection list/editor and credentials in Windows Credential
Manager. Full spec (split-pane tiling, tabs, SFTP, reconnect/backoff, packaging) is
deferred to later phases.

## Scaffold
- [x] `npm create tauri-app` (React + TS + Vite) at repo root, renamed from `tauri-app` to `sshmanager`
- [x] Tailwind v4 + shadcn/ui (base-ui backed), dark theme by default
- [x] Rust deps: `russh` (ring backend, not aws-lc-rs — avoids needing NASM on Windows), `keyring`, `uuid`, `tokio`, `thiserror`, `tauri-plugin-dialog`

## Backend (`src-tauri/src/`)
- [x] `storage/connections_store.rs` — JSON file CRUD, no secrets ever touch it
- [x] `secrets/keyring_store.rs` — Windows Credential Manager via `keyring`, keyed `sshmanager:<id>:password|passphrase`
- [x] `ssh/client.rs` — russh `Handler` + connect/auth (password or `russh::keys::load_secret_key`)
- [x] `ssh/pty.rs` — PTY + shell + resize + output streamed via `tauri::ipc::Channel<TerminalEvent>`
- [x] `commands/` — connections CRUD, credential save/has, session open/input/resize/close, test_connection

## Frontend (`src/`)
- [x] `ConnectionList` — search/filter, grouped by tag, connect/edit/duplicate/delete
- [x] `ConnectionEditorDialog` — add/edit, key file picker, password/passphrase field, Test Connect
- [x] `TerminalPane` — xterm.js + fit/search addons, wired to per-session `Channel`
- [x] `App.tsx` — sidebar + single active terminal pane shell

## Verification
- [x] `npm run tauri dev` builds and launches (had to disable the command sandbox — GUI window creation needs real window-station access)
- [x] Real connection added; password confirmed stored in Windows Credential Manager (`cmdkey /list`), not in `connections.json`
- [x] Live shell connect confirmed by user: password auth, `cd`/`ls` round-trip, no perceptible lag
- [x] Found + fixed: last terminal row clipped when full — padding was on the same div `term.open()` mounted into, so `FitAddon` overcounted rows. Moved padding to an outer wrapper.
- [x] Found + fixed: editing a connection's username/auth-type orphaned the old Credential Manager entry — now cleaned up in `save_connection`
- [x] User re-confirmed terminal fill/last-row fix after hot reload
- [ ] Key-auth (passphrase-protected private key) path — implemented, not yet exercised against a real key
- [ ] Test Connect against an unreachable host — implemented, not yet exercised
- [ ] Session cleanup on close/reopen (no leaked backend tasks) — implemented via `AppState.sessions` map removal, not yet stress-tested

## Deferred to Phase 2+
Split-pane tiling (`react-mosaic-component`) + tabs-of-grids, SFTP browser panel
(`russh-sftp`), reconnect-on-drop with cancellable backoff, packaging into a
standalone `.exe`/MSI.

## Review
Core MVP loop (add connection → save credential → connect → live shell) works
end-to-end against a real server, confirmed by the user, not just by compiling.
One real bug found and fixed during live testing (xterm row clipping); one
hygiene bug found by code review and fixed proactively (orphaned credentials).
Remaining checklist items are implemented but not yet independently exercised —
see lessons.md for why a couple of early missteps happened.
