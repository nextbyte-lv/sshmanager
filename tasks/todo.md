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

---

# Phase 2a — Split-pane tiling + tabs (multi-session workspace)

Plan: replace the single-active-connection shell with tabs, each holding its
own resizable `react-mosaic-component@6.2.0` tree of panes; panes addable via
split, closable, drag-to-rearrange. No backend changes needed — session
management was already multi-session-capable (`AppState.sessions` keyed by
UUID), this was purely an `App.tsx` state-model limitation.

## Frontend
- [x] `types/workspace.ts` + `lib/workspace.ts` — `Tab`/`PaneState` model, pure `createTab`/`splitPane`/`removePane` helpers
- [x] `PaneToolbar.tsx` + `ConnectionPickerMenu.tsx` — custom split/close controls (lucide icons, no Blueprint.js dependency)
- [x] `Workspace.tsx` — tab bar + per-tab `<Mosaic>`; **all tabs rendered simultaneously, inactive ones hidden via CSS (not unmounted)** so switching tabs doesn't kill background sessions
- [x] `TerminalPane.tsx` — guarded the `ResizeObserver` callback against the zero-size box a hidden (`display:none`) tab reports, so it doesn't collapse the PTY to 0×0 and correctly re-fits when the tab becomes visible again
- [x] `App.tsx` rewired: `tabs`/`activeTabId` state replaces `activeConnection`; `ConnectionList` connect action opens a new tab
- [x] Dark-theme CSS overrides for `.mosaic-window`/`.mosaic-split` (library ships hardcoded light-theme colors on these outside the Blueprint theme class)

## Verification
- [x] `tsc --noEmit` and `vite build` clean (watched specifically for react-mosaic-component's known React-19 `JSX.Element` `.d.ts` issue — silent under this project's existing `skipLibCheck`)
- [x] User confirmed: split and tab creation work; a live session survived a full app rebuild/relaunch
- [x] Found + fixed: pane drag-to-rearrange did nothing — Tauri's native OS-level drag-and-drop was intercepting the input before the DOM's `dragstart` ever fired. Fixed with `"dragDropEnabled": false` on the window in `tauri.conf.json`. User confirmed drag now works.
- [ ] Independently exercise: typing in one pane doesn't affect a sibling pane's session (implemented — each pane has its own session id — not yet explicitly stress-tested)
- [ ] Independently exercise: closing one pane only tears down that pane's backend session, siblings unaffected
- [ ] Independently exercise: closing a tab tears down every pane's session in it, no leaked backend tasks

## Deferred (unchanged from Phase 1)
SFTP browser panel (`russh-sftp`), reconnect-on-drop with cancellable backoff,
packaging into a standalone `.exe`/MSI.

**Note for the SFTP phase:** `dragDropEnabled: false` (added this phase) also
disables Tauri's native OS file-drop-onto-window event. If the SFTP panel's
drag-and-drop upload depends on that Tauri event rather than the DOM's own
`drop` handler, this will need reconciling then — see lessons.md.

## Review
The riskiest part of this phase wasn't the tiling library itself (worked on
the first real test) — it was two things I only found by having the user
actually use it: drag-and-drop being silently eaten by a Tauri default, and
(caught by design review before shipping, not yet independently confirmed)
the background-tab-must-stay-alive requirement, which needed a deliberate
"render all tabs, hide inactive" architecture rather than the more obvious
"only render the active tab" that would have silently killed background
sessions on every tab switch.

---

# Phase 2b — SFTP browser panel

Plan: toggle-able SFTP file browser per pane, reusing the same SSH connection
as that pane's terminal (no second connection/auth). Scope decision made with
the user: drag-and-drop upload from Explorer is downgraded to file-picker
buttons, since Tauri's native OS drag-and-drop and the Phase 2a pane-drag fix
(`dragDropEnabled: false`) can't both be on at once — buttons keep pane
dragging working.

## Backend (`src-tauri/src/`)
- [x] `russh-sftp = "2.3.0"` added
- [x] `state.rs`: sessions now store `SessionHandle { cmd_tx, ssh: Arc<Handle<Client>> }`; new `sftp: Mutex<HashMap<String, Arc<SftpSession>>>` cache, keyed by session id
- [x] `ssh/pty.rs::open` wraps the handle in `Arc` and returns it alongside the command sender, so the SFTP path can share the same connection (validated via source reading: `channel_open_session`/`disconnect` are both `&self`, so no `Mutex` needed; concurrent opens are safe, the connection driver task serializes them)
- [x] `ssh/sftp.rs` (new): `open_sftp`, `canonicalize`, `list_dir`, `download`/`upload` (whole-buffer, not chunked-streamed — a deliberate simplification for a personal tool, see below), `make_dir`, `remove`, `rename`
- [x] `commands/sftp.rs` (new): 7 commands, lazily open+cache one `SftpSession` per session id
- [x] `commands/session.rs::close_session` also drops the cached `SftpSession`

## Frontend (`src/`)
- [x] `types/sftp.ts`, `lib/tauri.ts` sftp* wrappers
- [x] `SftpPanel.tsx`: breadcrumb nav, listing, upload/download via native pickers, new folder/rename/delete
- [x] `TerminalPane.tsx`: `onSessionId` callback so a sibling panel can reuse the session id
- [x] `PaneLeaf.tsx` (new): folds terminal + toggleable `SftpPanel` + toolbar into one real component — `renderTile` is a plain callback and can't hold hook state (`sftpOpen`/`sessionId`) itself, so this needed to be a proper child component, not inlined
- [x] `PaneToolbar.tsx`: added SFTP toggle button

## Bugs found and fixed during this phase
- [x] **Navigation capped at home directory** — initial design used SFTP's
  relative-path shorthand (`.`) as the "root" for all navigation, so there was
  no way to express "go above home". Fixed by resolving `.` to a real absolute
  path via `sftp_canonicalize` once on mount, and switching all join/parent
  path logic to absolute paths. Not a permissions issue — the user correctly
  suspected it was a bug, not a "need a different account" situation.
- [x] Breadcrumb rendered a doubled-looking `/` at the start (root button showed
  `/` text, and the first segment's own separator also rendered `/` right next
  to it). Fixed by making the root button a distinct icon (not a `/` glyph) so
  it's visually unambiguous from the inter-segment separators.
- [x] SFTP panel's native-scrollbar file listing didn't match the dark theme —
  fixed globally in `index.css` (`::-webkit-scrollbar-*` + `scrollbar-color`),
  not scoped to just this component, per explicit request.

## Descoped
Full auto-sync between the terminal's `cd` and the SFTP panel's directory
would need OSC 7 escape-sequence reporting, which means injecting a visible
bash/zsh setup line into the shell right after connecting. User decided that
wasn't worth it for now. Shipped the reverse instead — a "cd here" button in
the SFTP panel that sends `cd '<path>'` into that pane's terminal — much
simpler (reuses existing `sendInput`), on-demand rather than automatic.

## Verification
- [x] `tsc --noEmit` / `cargo check` clean throughout
- [x] User confirmed: SFTP panel lists real remote directories, breadcrumb
  navigation works, can browse above the home directory after the fix
- [ ] Independently exercise: upload/download round-trip (file picker → confirm
  in listing → download to a different path → diff contents)
- [ ] Independently exercise: create folder / rename / delete against the real
  remote filesystem
- [ ] Independently exercise: pane drag-to-rearrange still works (regression
  check — this phase added more interactive elements inside each pane but
  didn't touch `dragDropEnabled`)

## Deferred (unchanged)
Reconnect-on-drop with cancellable backoff, packaging into a standalone
`.exe`/MSI.

## Review
Reused the Phase 1 pattern that worked well: verify the exact library/runtime
behavior by reading real source (`russh::client::Handle`'s `&self` vs `&mut
self` methods, `russh-sftp`'s actual API) before writing code against it,
rather than assuming from memory. The one design mistake (home-dir-capped
navigation) came from reaching for SFTP's relative-path convenience (`.`)
without thinking through what it does to "go up" semantics — worth remembering
for any future relative-path handling.
