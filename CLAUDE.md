# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Workflow Orchestration

### 1. Plan Mode Default

- Enter plan mode for ANY non-trivial task (3+ steps or architectural decisions)
- If something goes sideways, STOP and re-plan immediately
- Use plan mode for verification steps, not just building
- Write detailed specs upfront to reduce ambiguity

### 2. Subagent Strategy

- Use subagents liberally to keep main context window clean
- Offload research, exploration, and parallel analysis to subagents
- For complex problems, throw more compute at it via subagents
- One task per subagent for focused execution

### 3. Self-Improvement Loop

- After ANY correction from the user: update `tasks/lessons.md` with the pattern
- Write rules for yourself that prevent the same mistake
- Ruthlessly iterate on these lessons until mistake rate drops
- Review lessons at session start for relevant project

### 4. Verification Before Done

- Never mark a task complete without proving it works
- Diff behavior between main and your changes when relevant
- Ask yourself: "Would a staff engineer approve this?"
- Run tests, check logs, demonstrate correctness

### 5. Demand Elegance (Balanced)

- For non-trivial changes: pause and ask "is there a more elegant way?"
- If a fix feels hacky: "Knowing everything I know now, implement the elegant solution"
- Skip this for simple, obvious fixes -- don't over-engineer
- Challenge your own work before presenting it

### 6. Autonomous Bug Fixing

- When given a bug report: just fix it. Don't ask for hand-holding
- Point at logs, errors, failing tests -- then resolve them
- Zero context switching required from the user
- Go fix failing CI tests without being told how

---

## Task Management

1. **Plan First:** Write plan to `tasks/todo.md` with checkable items
2. **Verify Plan:** Check in before starting implementation
3. **Track Progress:** Mark items complete as you go
4. **Explain Changes:** High-level summary at each step
5. **Document Results:** Add review section to `tasks/todo.md`
6. **Capture Lessons:** Update `tasks/lessons.md` after corrections

---

## Core Principles

- **Simplicity First:** Make every change as simple as possible. Impact minimal code.
- **No Laziness:** Find root causes. No temporary fixes. Senior developer standards.
- **Minimal Impact:** Changes should only touch what's necessary. Avoid introducing bugs.

---

## What this project is

A personal SSH/SFTP session manager for Windows: Tauri v2 (Rust) backend +
React/TypeScript/Vite frontend, xterm.js for terminals, `russh` for SSH/SFTP,
Windows Credential Manager for secrets. Not a shipped product — no installer
signing, licensing, telemetry, or auto-update. See `README.md` for what's
built so far and where things are stored, `tasks/todo.md` for the phase-by-phase
build log, and `tasks/lessons.md` for concrete gotchas already hit and fixed
(read this before touching russh, react-mosaic-component, or Tauri's
drag-and-drop/window config — several non-obvious traps are documented there
with root causes, not just workarounds).

## Commands

```
npm install              # install frontend deps (pulls in @tauri-apps/cli)
npm run tauri dev         # dev server + app window, hot reload on frontend changes,
                           # rebuild+relaunch on Rust changes
npm run tauri build        # release build; installers land in
                           # src-tauri/target/release/bundle/
./node_modules/.bin/tsc --noEmit  # frontend typecheck only (NOT `npx tsc` --
                           # npx re-resolves and rewrites package.json; see tasks/lessons.md)
npm run build               # frontend typecheck + production Vite build (no Tauri)
cd src-tauri && cargo check  # backend compile check only (fast, no bundling)
```

There is no automated test suite yet — changes are verified by running the
app live (`npm run tauri dev`) against a real SSH session, not by unit tests.

## Architecture

**Process boundary:** the Rust backend (`src-tauri/src/`) owns every SSH/SFTP
connection, the credential store, and the connections list. The React
frontend (`src/`) only renders UI and calls Tauri commands over IPC — no raw
sockets, secrets, or filesystem access happen in the webview.

**Session sharing (`src-tauri/src/state.rs`, `ssh/pty.rs`, `ssh/sftp.rs`):**
each open terminal gets one `AppState.sessions` entry keyed by a generated
session id. That entry holds both the PTY's command channel *and* an
`Arc<russh::client::Handle<Client>>` to the underlying SSH connection. When a
pane's SFTP browser panel is opened, it reuses that same `Arc` to open an
*additional* channel on the *same* connection (no second TCP connection, no
second auth) — `AppState.sftp` lazily caches one `SftpSession` per session id
the first time it's needed. This only works because `Handle`'s
connection-opening methods take `&self` (verified by reading the `russh`
source, not assumed) — the one-time `authenticate_*` calls that need `&mut
self` all happen before the handle is wrapped in `Arc`.

**Frontend workspace model (`src/types/workspace.ts`, `src/lib/workspace.ts`,
`src/components/Workspace.tsx`, `PaneLeaf.tsx`):** `App.tsx` owns `tabs` +
`activeTabId`. Each `Tab` holds its own independent `react-mosaic-component`
tree (`Tab.layout`) plus a `panes` map from pane id to connection id;
`lib/workspace.ts` has the pure `createTab`/`splitPane`/`removePane` tree
helpers. **All tabs render simultaneously; inactive ones are hidden via CSS
(`display:none`), never unmounted** — unmounting would tear down every
background tab's SSH sessions on every tab switch. `PaneLeaf.tsx` exists as a
real component (not inlined in `Mosaic`'s `renderTile` callback) specifically
so it can hold hook state (`sftpOpen`, the terminal's `sessionId`) that a
plain render-prop callback isn't allowed to.

**Host monitor (`src-tauri/src/ssh/monitor.sh`, `ssh/monitor.rs`,
`commands/monitor.rs`, `src/components/MonitorPanel.tsx`):** the per-pane task
manager samples the host with **one** exec channel per poll on the session's
existing connection. `monitor.sh` is a single POSIX collector emitting `@@name`
sections and a final `@@end` sentinel; it is sent as channel **stdin** to a bare
`sh`, never as `sh -c '<script>'`, because the login shell may be fish or csh and
both reinterpret characters inside single quotes that the script's awk depends on.
Exit status is ignored (`cat` over `/proc/[0-9]*/stat` exits nonzero whenever a pid
vanished mid-read); the sentinel is the completeness check.

Every interesting figure is a **counter delta**, so `AppState.monitor` keeps the
previous `RawSample` per session id behind an `Arc<tokio::sync::Mutex<..>>` (async,
because it is held across the collection await) and `monitor::diff` subtracts. That
cache must be invalidated everywhere `AppState.sftp` is — `close_session` plus
*both* reconnect branches in `ssh/pty.rs` — or the first sample after a host reboot
subtracts large old counters from small new ones. The frontend polls with a chained
`setTimeout` after the await (never `setInterval`) and skips the poll when
`offsetParent` is null, which is exactly when its tab is hidden. All the /proc
parsing lives in pure functions with tests, including a captured real-sample pair
in `ssh/testdata/` — the traps here produce plausible wrong numbers rather than
errors, and `tasks/lessons.md` lists them.

**Credentials (`storage/connections_store.rs`, `secrets/keyring_store.rs`):**
connection metadata (host/port/username/tags) is a plain JSON file at
`%APPDATA%\<identifier>\connections.json` and never contains secrets.
Passwords/passphrases live in Windows Credential Manager via the `keyring`
crate, keyed `sshmanager:<connection-id>:password|passphrase`.

**Windows-specific build/runtime traps** (full detail + fixes in
`tasks/lessons.md`):
- `russh`'s default crypto backend (`aws-lc-rs`) needs NASM to build on
  Windows; this project uses the `ring` feature instead.
- `russh-keys` is a separate, version-incompatible crate — russh 0.62+ has
  its own built-in `russh::keys` module; don't add `russh-keys` back.
- `tauri.conf.json` sets `dragDropEnabled: false` on the window. Without
  this, Tauri's native OS-level drag-and-drop swallows the DOM `dragstart`
  event before `react-mosaic-component`'s pane-drag-to-rearrange ever sees
  it. Side effect: native OS file-drop-onto-window is unavailable, which is
  why SFTP upload uses file-picker dialogs instead of drag-and-drop from
  Explorer.
- Terminal output streams backend→frontend via one `tauri::ipc::Channel` per
  session (not `emit`/`emit_to`) — deliberate, for per-session ordering and
  throughput.

## Frontend conventions

Prefer a reusable component over a raw HTML element for anything that's a
recognizable UI control — buttons, text/checkbox/radio inputs, selects. If
`src/components/ui/` doesn't already have it, add it (`npx shadcn@latest add
<component>`) rather than hand-rolling the markup/styling inline. This keeps
styling and behavior (focus rings, disabled states, dark theme) consistent
and in one place instead of copy-pasted across dialogs. Raw tags are fine for
plain structural/layout elements (`div`, `span`, `p`) and for a one-off hit
target whose look is fully custom (e.g. a plain-text clickable row) — the
rule is about not re-implementing a *control*'s look and behavior inline, not
banning HTML tags outright.
