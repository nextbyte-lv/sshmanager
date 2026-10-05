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

---

# SFTP file permissions (chmod)

## Backend (`src-tauri/src/`)
- [x] `ssh/sftp.rs`: `SftpEntry` now carries `mode` (masked to `MODE_BITS`
  = `0o7777`, type bits stripped), `is_symlink`, `uid`, `gid` — all of it already
  present in the `readdir` attributes, so the listing costs no extra round-trips
- [x] `ssh/sftp.rs::set_mode` — `setstat` with only the `permissions` field set
- [x] `ssh/sftp.rs::set_mode_recursive` — `chmod -R` over SFTP, which has no
  recursive setstat. Walks the whole tree *first*, then applies to files and to
  directories deepest-first; symlinks skipped (setstat follows them)
- [x] `ssh/mod.rs`: `SshError::RemoteChmod { path, source }`, wired into
  `is_permission_denied` so the sudo retry can trigger on it
- [x] `commands/sftp.rs::sftp_set_mode` — validates the mode carries no type bits,
  refuses a recursive run anchored at `/`, falls back to `elevated_chmod`
  (`sudo chmod [-R] NNNN -- path`) when and only when the server said permission
  denied, exactly like the existing write/delete escalation

## Frontend (`src/`)
- [x] `lib/permissions.ts` (new): mode is one number; octal parse/format
  (3 digits, 4 once a special bit is set), `ls -l` symbolic rendering with
  `s`/`S`/`t`/`T`, bit get/set helpers, the four presets
- [x] `components/PermissionsDialog.tsx` (new): octal field + 3x3 r/w/x grid +
  setuid/setgid/sticky + presets + "apply to everything inside this folder" for
  directories, all views of the same number. Failures show in the dialog and it
  stays open
- [x] `SftpPanel.tsx`: each row shows its mode as clickable octal (tooltip has the
  symbolic form), plus a lock button in the hover actions; both open the dialog

## Verification
- [x] `cargo check` + `cargo test` clean; new test pins the escalated command line
  (`chmod -R 0755 -- '/srv/site'`) — mode is text by the time it reaches a shell,
  and `644` vs `0644` vs a dropped special bit are three different outcomes
- [x] `npx tsc --noEmit` + `npm run build` clean
- [x] `lib/permissions.ts` checked against real `ls -l` output for 11 modes
  (incl. `1777`→`drwxrwxrwt`, `1666`→`drw-rw-rwT`, `4655`→`-rwSr-xr-x`) and the
  parse/edit helpers for octal round-trips and rejected input
- [ ] Exercise live: mode column shows real modes; change a file you own; change a
  root-owned file (expect the sudo path); recursive on a small tree; a mode the
  server refuses outright (expect the message in the dialog, dialog stays open)

## Descoped
Owner/group (`chown`). Ownership isn't mode, a non-root user can't give a file
away so it would be a sudo-only feature, and SFTP's `setstat` only speaks numeric
uid/gid. Worth adding to the same dialog later if it comes up.

## Folder sizes in the SFTP panel

SFTP has no "size of a directory" — `read_dir` returns the directory inode's own
size (a few KB), which is why the panel showed sizes for files only. A real
folder size means summing the tree, so it is computed on demand over an exec
channel with `du`, never as part of a listing (a listing of `/` would stat every
file on the machine before the panel could paint).

- [x] `ssh/exec.rs`: capture stdout as well as stderr in `ExecOutput`
- [x] `ssh/sftp.rs`: `dir_sizes()` — one batched `du -sb`, `du -sk` fallback for
      non-GNU `du`, per-path result carrying a `partial` flag when `du` couldn't
      read part of the tree
- [x] `commands/sftp.rs`: `sftp_dir_sizes` command + registration in `lib.rs`
- [x] `types/sftp.ts` + `lib/tauri.ts`: `DirSize`, `sftpDirSizes()`
- [x] `SftpPanel.tsx`: per-folder click-to-calculate in the size column, and a
      toolbar button that sizes every folder in view in a single round trip

### Review

Sizes are cached per directory listing and cleared on navigate/refresh so a
number never outlives the tree it measured. A partial result (unreadable
subdirectory) renders with a `~` and says so on hover rather than silently
under-reporting. No sudo escalation here on purpose: unlike a write or a chmod,
`du` failing on part of a tree still returns a usable number, and `sudo du -sb`
over an arbitrary path is a lot of privilege for a display nicety.

## Upload: visible failures, and sudo escalation

An upload of a folder into a root-owned parent reported nothing at all — no
progress, no error, no notification. Two separate silences, then one missing
feature behind them.

- [x] `SftpPanel.tsx`: split the panel's single `error` state into `listError`
      (owned and cleared by `refresh()`) and `actionError` (owned by the user's
      last action). `uploadPaths()` ends with `refresh()`, whose first act was
      `setError(null)` — it wiped the very message the upload had just set
- [x] `SftpPanel.tsx`: upload failures are collected (channel `file_error`
      events *and* a rejected `sftp_upload`) and reported as a summary when the
      transfer ends, with a count when there is more than one
- [x] `SftpPanel.tsx`: `openFileDialog`/`saveFileDialog` calls moved inside a
      `try` — awaited bare in a click handler, a rejecting picker was an
      unhandled rejection, i.e. a button that does nothing
- [x] `ssh/sftp.rs`: `Elevate` — the walk hands the one operation the server
      refused back to its caller instead of growing an exec channel and
      credentials of its own; `upload_path` takes the retry as a callback
- [x] `commands/sftp.rs`: `elevated_mkdir` (`sudo mkdir -p`) and `elevated_put`
      (stage to `/tmp` over SFTP, `sudo cp` into place, always clean up the
      staging file), wired into `sftp_upload`
- [x] `ssh/sftp.rs`: a refused `create_dir` now carries the server's own reason
      instead of a flat "failed to create remote directory"

### Review

Escalation follows the rule the write, delete and chmod paths already set: retry
only a `PermissionDenied` refusal, never a generic failure, so nothing gets a
second run as root on the strength of an unrelated error. Uploads escalate
per file rather than as one `sudo cp -r` of a staged tree — more round trips,
but the progress events and the size+mtime skip check stay per file, and a tree
where only one directory is root-owned doesn't get wholesale root treatment.

Known cost, deliberate: `cp` (not `cp -p`) keeps an existing target's inode,
owner and mode, but leaves the mtime as *now*, so an escalated file is re-sent
on the next upload instead of being skipped. Preserving it would mean either
handing the target's ownership to the login user or a GNU-only flag.

- [ ] Exercise live: upload a file and a folder into a root-owned directory
      (expect success via sudo); the same with a key-only connection and no
      password-less sudo (expect "…and sudo could not either: sudo: a password
      is required" in the panel); cancel a picker (expect no message at all)

# Remote task manager (per-pane host monitor)

A bottom-docked panel per pane: CPU (model, load, steal/iowait), RAM/swap,
filesystem usage, network throughput, and a sortable process list with *true
instantaneous* CPU% — plus kill/signal, sparklines, a filter box, a refresh
interval selector, and a listening-ports tab. Rows that move or appear flash
green and fade. Linux `/proc` only; anything else says so rather than showing
plausible-looking wrong numbers.

Full design (with the arithmetic and the list of traps that produce silently
wrong numbers) in `C:\Users\arccuks\.claude\plans\merry-singing-duckling.md`.

## Backend — collection (`src-tauri/src/ssh/`)

- [x] `monitor.sh` — one POSIX collector, `include_str!`ed, sent on **stdin** to
      a bare `sh` rather than as `sh -c '<script>'`: the login shell may be fish
      (which escapes `\` inside single quotes) or csh (which history-expands `!`
      inside them), and an awk program parsing `/proc` is full of both
- [x] `monitor.sh` — `@@name` section delimiters plus a final `@@end` sentinel;
      no `set -e`; `export LC_ALL=C`; per-section `2>/dev/null`
- [x] `monitor.rs` — parsers for `/proc/stat`, `meminfo`, `net/dev`, `uptime`,
      `loadavg`, `cpuinfo`, `df -P -k -l`, `mounts`, the projected process table
      and `ps`
- [x] `monitor.rs` — `diff(prev, curr)`: htop's CPU decomposition, per-process
      CPU% against the aggregate jiffy delta, `(pid, starttime)` keying,
      `checked_sub` with discard-on-negative
- [x] `monitor.rs` — degraded-data warnings: cgroup-limited container, `hidepid`,
      missing `MemAvailable`, missing `ps`
- [x] `monitor.rs` — `#[cfg(test)]` tests over two captured consecutive samples

## Backend — commands (`src-tauri/src/commands/`)

- [x] `sftp.rs` — open up `session_ssh`, `run_with_sudo`, `shell_quote`,
      `last_error_line` as `pub(crate)` instead of duplicating them
- [x] `monitor.rs` — `monitor_sample`, wrapped in `tokio::time::timeout`, with
      per-session `Arc<tokio::sync::Mutex<MonitorState>>` so overlapping polls
      cannot diff against each other's sample
- [x] `monitor.rs` — `monitor_kill` with a `starttime` re-check before signalling
      and a refusal for pid 1; sudo fallback on refusal
- [x] `monitor.rs` — `monitor_ports` (`ss -H -tulpn`, `netstat` fallback)
- [x] `state.rs` — the `monitor` map, invalidated everywhere `sftp` is:
      `close_session`, and *both* reconnect branches in `ssh/pty.rs`
- [x] `lib.rs` — register the three commands

## Frontend (`src/`)

- [x] `types/monitor.ts`, `lib/tauri.ts` bindings, `lib/monitor.ts` pure helpers
      (formatting, sort comparators, `movedPids`)
- [x] `hooks/useResizablePanel.ts` — vertical directions (`grow-up`/`grow-down`)
- [x] `components/PaneToolbar.tsx` + `PaneLeaf.tsx` — the toggle and the dock
- [x] `components/MonitorPanel.tsx` — chained-`setTimeout` polling (never
      `setInterval`, so a slow host slows the rate instead of queueing), gated on
      `offsetParent === null` so a panel in a hidden tab stops polling
- [x] `components/monitor/` — `MonitorStats`, `Sparkline` (inline SVG, no new
      dependency), `ProcessTable`, `PortsTable`
- [x] `index.css` — `--flash` tokens for both themes; the palette is monochrome
      by design, so no hardcoded `emerald-500`
- [x] `ui/table.tsx`, `ui/progress.tsx` via `./node_modules/.bin/shadcn add`
      (never `npx shadcn` — it rewrites `package.json` and the lockfile)

## Verification

- [x] `cargo check` (clean, no warnings), `cargo test` (26 passing),
      `./node_modules/.bin/tsc --noEmit`, `npm run build`
- [ ] Live against a real Linux host, side by side with `htop`/`free -m`/`df -h`
      in the terminal directly above the panel: total CPU%, top processes' CPU%
      and RSS, used/available RAM, each filesystem's percentage
- [ ] Generate load (`yes > /dev/null` xN, `dd`, a large `scp`) and confirm the
      numbers move and settle; a multi-threaded process reads >100% per-core
- [ ] Leave it open ~10 min: no drift, no NaN, no frozen card. Force a reconnect
      and confirm the first sample after it is discarded, not shown as a spike
- [ ] Flash: sorted by PID only genuinely moving/new rows flash; sorted by CPU%
      judge strobe-vs-signal and tune
- [ ] Kill one of your own processes, then a root-owned one; on a key-auth
      connection confirm it reports honestly instead of sending the passphrase
      to sudo. Confirm pid 1 is refused
- [ ] Two hosts plus one background tab: numbers stay per-host, hidden tab stops
      polling

## Verified without a server, against WSL

The collector and every parser were exercised against a real Linux `/proc` before
any of this went near the app, by piping `monitor.sh` into
`wsl.exe -d Debian -- sh` exactly as `exec::run` will. That caught three real
defects that reading the code would not have: a shell redirect that failed before
`tr` ever ran (leaking to stderr), twenty tmpfs mounts crowding the real volumes
off the disk card, and `ps` column padding the row parser mishandled.

Two consecutive samples were then captured two seconds apart with a known
`yes > /dev/null` running, and kept as `src-tauri/src/ssh/testdata/sample{1,2}.txt`.
Against that real pair the code computes `yes` at **99.34% of one core** and the
machine at **8.48% of 24 cores** — which is exactly what one saturated core out of
twenty-four is. That single assertion pins the whole per-process CPU chain: the
awk field offsets, the aggregate-jiffy denominator, the per-core scaling, and the
`(pid, starttime)` keying. It is a regression test now.

## Review

Shape of it: one exec round trip per poll running `monitor.sh` (sent on **stdin**
to a bare `sh`, not as `sh -c '<script>'` — see `tasks/lessons.md`), with Rust
holding the previous raw sample per session and subtracting. The panel polls with
a chained `setTimeout` after the await, so a slow host slows the rate instead of
queueing samples, and skips the poll entirely when `offsetParent` is null — which
is exactly when its tab is hidden, and tabs are never unmounted here.

Deliberate calls worth knowing:

- **Linux only.** A non-Linux host gets a plain refusal rather than partial
  numbers. Everything in `/proc` is exact and one round trip; a sysctl/vm_stat
  collector for BSD/macOS would be a second implementation, not a patch to this.
- **Per-core CPU% is what is stored** (htop's and top's scale: four busy cores
  read 400), with the whole-machine reading derived in the UI behind a toggle. An
  unlabelled `380%` reads as a bug, so the footer always says which is showing.
- **No total for disk I/O.** `/proc/diskstats` lists `sda`, `sda1` and `dm-0`
  alike, so any sum double- or triple-counts. Per-device rows only, filtered to
  what `/sys/block` calls a real device.
- **A degraded-data badge**, because the dangerous failures here are the ones that
  still look plausible: inside a cgroup-limited container every `/proc` figure is
  the *host's*, and under `hidepid=2` the process list comes back nearly empty.
  Both are detected from data the sample already collects.
- **Kill re-checks `starttime` before signalling** and refuses pid 1. Without
  that, a pid recycled between drawing a row and clicking it kills something
  unrelated — the one way this feature could do real damage.
- **Two error slots, not one** (`listError` owned by the poll, `actionError` by the
  user's last action), following the lesson the SFTP panel already paid for.

Known cost, accepted: the sample shares the terminal's TCP connection, ~90-100 KB
per poll on a 500-process host. Mitigated by truncating `args` on the host to 200
characters, capping the table at 300 rows, the 1s/2s/5s/paused selector, and
pausing in background tabs — but on a slow link a 1s interval will be noticeable
while typing, and 5s is the better choice there.

Dropped from the plan after building it: `React.memo` on the rows. Every row's CPU
value changes every tick, so it would never hit; capping the rendered rows is the
optimisation that actually does something.

- [ ] **Open question for live use — the green flash.** It fires on a row whose
      sort position changed or that is new, as specified. Sorted by PID, name,
      user or memory that is a genuine signal. Sorted by CPU% at a 2s poll, most
      rows move most ticks, so it may read as strobing rather than informing.
      There is an on/off toggle in the panel toolbar; if it does prove noisy, the
      cheap next step is narrowing it to *new* pids only, which are rare and
      always interesting. Judge it on the real thing before changing it.

## Per-process network usage, and finding a process that hides

Goal: "which process is using the network", so a shady program on a server is
easy to find. Per-process throughput on Linux has no single source — the
attribution comes from socket inodes, and the bytes from `tcp_info`.

Verified against a live kernel before designing any of it (see the WSL note
below): `ss -tinep` reports per-socket cumulative `bytes_sent`/`bytes_received`
that advance between polls, a stable `ino:` key to diff them by, and
`users:(("name",pid=N,fd=M))` attribution. **`/proc/<pid>/net/dev` is a trap and
is not used** — it is per network *namespace*, so every process in the root
namespace reports identical host-wide totals that look convincingly per-process.

Two hard limits, both surfaced in the UI rather than hidden:
- **Byte counters are TCP only.** `tcp_info` has no UDP equivalent, so a UDP
  beacon can be listed but not measured.
- **Attributing another user's socket, or reading another user's
  `/proc/<pid>/exe`, needs root.** Unprivileged you get the connection and its
  uid but not the process. Hence the opt-in sudo button, agreed with the user:
  off by default, one sudo'd command per refresh only while it is on.

- [x] `monitor.sh` — `@@sockets` from `ss -H -tunaep -i state connected` (the
      state filter drops listeners and TIME_WAIT, which carry no bytes; verified
      0 rows vs 7 for all states on an idle host)
- [x] `monitor.sh` — `@@exe` from one `ls -l /proc/[0-9]*/exe` (1 ms for every
      pid on the host), awk-projected to `pid path [(deleted)]`
- [x] `monitor.rs` — `RawSocket` parsing: `ss` emits two lines per socket, the
      `-i` counters on a tab-indented continuation line
- [x] `monitor.rs` — socket byte deltas keyed by `ino`, summed per pid into
      per-process rx/tx rates
- [x] `monitor.rs` — peer address classification (loopback / private / public) so
      outbound-to-the-internet stands out
- [x] `monitor.rs` — exe path per process, `(deleted)` flag, and a flag for
      world-writable directories (`/tmp`, `/dev/shm`, `/var/tmp`): argv is
      attacker-controlled, the exe symlink is not
- [x] `commands/sftp.rs` — `run_with_sudo_output`, so the sudo mechanics stay
      single-sourced now that a caller needs the stdout as well as the status
- [x] `commands/monitor.rs` — `elevated` flag on `monitor_sample`: re-runs just
      the two privileged lookups under sudo and merges them over the
      unprivileged sample
- [x] Frontend — sortable `Net down`/`Net up` columns, a Connections tab, the
      elevated toggle, and a badge when attribution is incomplete
- [x] Verify against WSL with real TCP traffic: a known transfer rate must land
      on the right pid

### Review

The attribution and the bytes come from different places and are joined here:
`ss -e` gives each socket's **inode**, which is the only identifier stable across
polls (pids and ports are both recycled), `ss -i` gives tcp_info's cumulative
byte counters, and `ss -p` gives the owning pid. Rust diffs the counters by inode
and sums them onto the pid, so the process table's `Net down`/`Net up` columns
are real per-process throughput rather than a share of a host-wide number.

Proven against a live host rather than asserted: the fixture pair in
`ssh/testdata/` was captured with a sender throttled to 64 KiB every 62.5 ms —
1.024 MB/s, worked out *before* the capture — and the code reads **1,038,194 B/s**
on the right pid, within 1%. The same pair carries a process running from a
`/tmp` binary it had already deleted, so both hunting signals are regression
tested too.

Limits, all stated in the UI rather than papered over:
- **TCP only.** tcp_info has no UDP equivalent, so a UDP socket is listed with no
  rate. The Connections footer says so.
- **Sockets that open and close between two refreshes are not counted.** Their
  bytes existed but no baseline ever saw them. Counting those needs packet
  capture (what nethogs does), which needs root and a sniffer on the host.
- **Naming another user's process needs root**, for sockets and for
  `/proc/<pid>/exe` alike — hence one shield toggle covering both, off by
  default. A refused escalation degrades to the unprivileged sample plus a
  warning, rather than failing the whole poll.
- The toggle is deliberately *not* auto-disabled when sudo keeps refusing: the
  warning says why and the user turns it off. Silently reverting a switch someone
  set is worse than a visible failure, though it does mean a sudo entry per poll
  until they do.

- [ ] Live check once a real host is available: turn the shield on for a host
      where sudo needs a password, confirm attribution appears; then on a
      key-auth connection with no passwordless sudo, confirm it degrades to the
      warning instead of sending the key passphrase anywhere

---

## Phase: physical memory modules (DDR type, size, speed, part number)

The panel's Memory card reads `/proc/meminfo`, which knows how much RAM the host
has and nothing about the sticks it is made of. Module type/speed/part number is
SMBIOS/DMI data, so it needs a different source, a different privilege level, and
— being static hardware — a different cadence from the 2-second poll.

- [x] `ssh/dimms.sh` — one-shot collector: EDAC sysfs (unprivileged, but only
      present on ECC-capable hardware) and the raw DMI table via
      `od -An -v -tx1`, in one round trip
- [x] `ssh/dimms.rs` — SMBIOS type 17 (Memory Device) and type 16 (Physical
      Memory Array) parsed in Rust, so `dmidecode` need not be installed;
      `parse`/`merge_privileged`/`inventory` in the shape `monitor.rs` uses
- [x] `monitor.rs` — `split_sections` to `pub(crate)`, one section splitter for
      both collectors
- [x] `commands/monitor.rs` — `monitor_memory_modules`: unprivileged first, and
      escalate for the DMI table only when nothing came back, so a host that
      answers via EDAC never triggers a sudo entry
- [x] Frontend — a "Modules" button on the Memory card, a summary line once
      read, and a dialog with the per-slot table
- [x] Verify: parser tests against a captured real DMI table; collector run
      against WSL


### Review

`/proc` has no DIMM inventory of any kind, so this reads the firmware's own
SMBIOS structure table (`/sys/firmware/dmi/tables/DMI`) and parses it in Rust
rather than shelling out to `dmidecode`, which is missing from most minimal
container and cloud images. `od -An -v -tx1` carries the bytes back: POSIX,
in busybox, and no decoder crate for 5 kB. `-v` is not optional — 64 zero bytes
come back as 17 tokens without it and 64 with, and a memory table is mostly
padding.

Proven rather than asserted: `ssh/testdata/dmi-ddr5.txt` is a real SMBIOS 3.6
table from a live machine, and it is **byte-identical to what GNU `od` on a real
Linux host prints for those bytes** (checked by feeding the table back through
WSL's own `od` and diffing). So the parser tests run on exactly the stdout the
collector produces. Ground truth for the fixture — 4 × 16 GB Kingston DDR5-5600,
non-ECC, 128 GB maximum — came from the same machine's `Win32_PhysicalMemory`,
independently of the code under test. Serial numbers and asset tags were
overwritten with `X` of identical length before committing, so every offset in
the table is still the one the firmware wrote, and a test asserts neither field
reaches the struct.

Design decisions worth keeping:
- **Not on the poll.** DIMMs do not change while a host is up, and the table is
  mode 0400. Reading it on panel open would spend a sudo entry on someone who
  came for a CPU graph, so it is one button and the answer is kept until the
  session id changes.
- **Escalate only when both sources came back empty.** The collector tries EDAC
  sysfs and the DMI table in one round trip; a host that answered either way
  never triggers sudo.
- **Field reads are bounded by each structure's own `length` byte**, not by the
  newest spec's layout — a pre-2.7 table simply stops before `configured speed`
  exists. There is a test that cuts the real modules back to SMBIOS 2.3 and
  asserts fields go missing rather than bytes being read past the end.

Limits, stated in the UI rather than papered over:
- **A VM usually publishes nothing useful.** QEMU/KVM, VMware and the cloud
  hypervisors synthesise a single fake device with type `Other`/`RAM` and no
  manufacturer. The failure message says so instead of implying a broken read.
- **EDAC is the only unprivileged source and it is much poorer** — type and size,
  no speed, manufacturer or part number — and it exists only where an EDAC driver
  bound to the memory controller, i.e. ECC-capable hardware. When it is what
  answered, the dialog says which source it came from.
- Serial numbers are deliberately not read. They identify the machine and answer
  nothing anyone opens this panel for.

- [ ] Live check once a real host is available: a bare-metal Linux box with
      passworded sudo (expect the full table) and a VPS (expect the "publishes no
      memory module table" message, not a spinner)

---

# Upload: one toolbar button, and a real progress denominator

Two complaints, one about the toolbar and one about the progress line, both
landing on the same code path.

## One upload button instead of two

The backend never had this split — `sftp::upload_path` takes any local path and
recurses if it is a directory, so files and folders have always been one
operation. The two buttons existed because the **OS file dialog** is the thing
that cannot do both: the Windows common dialog is either a multi-select file
picker or a folder picker, and Tauri's `open({ directory })` is a straight
passthrough. Drag-and-drop from Explorer would cover both in one gesture, but it
is deliberately disabled (`dragDropEnabled: false`, needed for mosaic pane-drag).

So the merge is one *button* opening a dropdown, not one dialog.

- [x] Collapse the two toolbar buttons into one `Upload` trigger with a
      `DropdownMenu` — "Files…" / "Folder…", each calling `pickAndUpload`

## "12 / 340 files" instead of "1 uploaded"

`upload_path` discovers files lazily off a stack, so nothing knows the total
until the walk is over. The panel could only count events after the fact: no
denominator, and a bar measuring the *current file's* bytes that restarted on
every file — useless for a folder, and the reason speed/ETA jittered.

The denominator has to be established before the transfer, and it has to cover
the **whole selection** rather than one path at a time: the frontend calls
`sftpUpload` once per selected path, so a per-path total would grow mid-transfer.
Hence one local-only pre-scan across every selected path, not a `Scanned` event
inside `upload_path`.

- [x] `ssh/sftp.rs`: `scan_local_paths` — local `tokio::fs` walk returning
      `{ files, bytes }`, following symlinks exactly as `upload_path` does so the
      count it produces is the count the upload will reach
- [x] `UploadEvent::Skipped` carries `total_bytes` (`local_size` is already in
      hand) — without it a skipped file freezes the overall bar
- [x] `commands/sftp.rs`: `sftp_scan_local` command (no SSH involved)
- [x] `SftpPanel.tsx`: scan once up front, then track overall bytes as
      `completed + current file` and render `n / total files`; speed and ETA move
      onto the overall figure

Failed subtrees leave the bar short of 100% on purpose — the summary line already
names the failures, and inventing progress for files that never transferred would
be the lie.

## Review

Done and checked: `cargo test` 47 passed (4 of them new, on the scan), the
frontend typechecks, and `npm run build` is clean. Not yet run against a live
host — that is the one step left.

What the change actually turned on:
- The upload button is one `DropdownMenu` ("Files…" / "Folder…"). `disabled` sits
  on the *Trigger*, not on the `Button` inside `render` — base-ui's Trigger has a
  `disabled` prop of its own, and letting the render target set it instead is how
  a menu stays openable mid-transfer.
- The bar measures the whole selection instead of the file in flight. Progress is
  `baseBytes + fileBytes`, where a file's own size moves into `baseBytes` only
  when that file ends. `inFlightBytes` is null between files precisely because a
  `file_error` can name a *directory* — a failed `create_dir` or `read_dir` — and
  without the null that would bank the previous file's size twice.
- Speed and ETA are now sampled over the whole transfer. The old sample reset on
  every `started`, which is why a folder of small files showed a rate that never
  settled: each file threw the estimate away just as it was becoming meaningful.
- Tests pin the invariant that matters, which is not "the scan works" but "the
  scan counts what the upload will move": directories excluded, multiple roots
  summed, an unstattable root left out of the total the same way `upload_path`
  puts it under `failed`. Any drift there is a bar that sticks at 94%.

Deliberately left alone: `upload_path` still walks lazily and still runs once per
picked path. The scan is a second, local-only walk rather than a `Scanned` event
from inside the transfer — one command per path would have emitted one total per
path, and a denominator that climbs while the bar fills is worse than none.
