# Lessons

## Scaffolding into a non-empty directory can silently delete existing files
`npm create tauri-app@latest -- "." --force` wiped `CLAUDE.md` from the repo root
without warning. `--force` on create-tauri-app means "create even if not empty,"
not "merge" — it can overwrite/clobber. Recovered by re-writing the file from the
system-reminder content shown at the start of the conversation, but that's luck,
not process.
**Rule:** before running any scaffolding/generator tool against a non-empty
directory, back up or move aside existing files first (or scaffold into a temp
dir and merge by hand).

## Don't trust similarly-named crates to be a matched pair
`russh` 0.62.2 and `russh-keys` 0.49.2 look like the obvious pairing (same
project, "keys" companion crate) but are different generations: russh 0.62
folded key-handling into its own `russh::keys` module and pins `ssh-key =
"=0.7.0-rc.11"`, while the standalone `russh-keys` 0.49.2 pins `ssh-key = "0.6"`.
Their `PrivateKey` types are incompatible despite identical names. Would have
caused confusing type errors deep into writing client code.
**Rule:** before depending on a "companion" crate, check whether the main crate
already re-exports the functionality internally (`grep` its `lib.rs`/`mod.rs` for
`pub mod`/`pub use`), and check the version pins actually line up
(`cargo tree`, read both `Cargo.toml`s) rather than assuming same-repo naming
implies compatibility.

## `aws-lc-sys` needs NASM on Windows MSVC — prefer a `ring` feature if offered
Default-featured `russh` pulls `aws-lc-rs` → `aws-lc-sys`, whose build script
panics with "NASM command not found" on a fresh Windows machine with no NASM
installed. `russh` exposes a `ring` feature as an alternative crypto backend
that has no such requirement.
**Rule:** on a Windows-targeted Rust project, run `cargo tree -i aws-lc-sys`
after adding any TLS/crypto-adjacent crate; if present and a pure-`ring` (or
similar no-external-toolchain) feature exists, switch to it rather than asking
the user to install NASM.

## This environment's default Bash sandbox can't create real GUI windows
Launching `npm run tauri dev` under the default (sandboxed) Bash tool compiled
and ran cleanly, logged `Running target\debug\sshmanager.exe`, then the whole
process exited with code 0 and no error — no window ever appeared. Re-running
the identical command with `dangerouslyDisableSandbox: true` produced a real,
visible window (`Get-Process` showed a `MainWindowTitle`).
**Rule:** a GUI process that "completes" (exit 0) immediately after its
"Running `<exe>`" log line, with no crash/panic output, is the signature of
missing window-station/desktop access in the sandbox — not an app bug. Retry
with `dangerouslyDisableSandbox: true` before debugging the app itself.

## xterm.js: never put CSS padding on the element passed to `term.open()`
`FitAddon` measures the mounted element's `clientHeight`/`clientWidth`, which
*includes* that element's own padding, but the xterm canvas only fills the
element's content box (padding excluded). Padding on the mounted div makes
`FitAddon` overcount rows/cols by the padding amount, so the last row/column
renders clipped — but only once the terminal is actually full, so it's easy to
ship and only notice under real use (exactly how this shipped: found via live
user testing, not the initial smoke test).
**Rule:** always mount `term.open()` into an unpadded inner div; put any visual
padding on an *outer* wrapper around it.

## Tauri's built-in native drag-and-drop swallows HTML5 DnD before the DOM sees it
Added `react-mosaic-component` for pane tiling; splitting/closing panes (plain
`onClick`) worked immediately, but dragging a pane's title bar to rearrange it
did nothing — `MosaicWindow`'s drag is wired through `react-dnd`'s
`HTML5Backend` (native browser `draggable`/`dragstart`/`drop` events). Root
cause: Tauri windows have their own OS-level drag-and-drop (for native file
drops) enabled by default, and it intercepts the drag input before it ever
reaches the webview's DOM, so `dragstart` never fires.
**Rule:** if native HTML5 drag-and-drop (not pointer-based custom drag)
doesn't fire at all inside a Tauri window — dragstart never triggers, no JS
errors — set `"dragDropEnabled": false` on that window in `tauri.conf.json`
(`app.windows[].dragDropEnabled`) before assuming it's a library/webview bug.
**Caveat to revisit:** this also disables Tauri's native OS file-drop-onto-
window event. If the planned SFTP panel drag-and-drop upload
(drag a file from Explorer onto the app) turns out to depend on that Tauri
event rather than the DOM's own `drop` handler, this setting will need
reconciling — check when building that feature.
**Resolved:** when building the SFTP panel, downgraded drag-and-drop upload to
file-picker buttons specifically to avoid re-enabling `dragDropEnabled` and
re-breaking pane drag. Decided with the user rather than silently trading one
already-shipped feature for another.

## Relative-path shorthand (`.`) makes a bad navigation root
Built the SFTP browser's directory navigation using SFTP's `.`
(server-resolves-relative-to-login-directory) as the starting/"root" path,
with an explicit ceiling: going "up" from `.` just returned `.` again. Result:
no way to navigate above the home directory at all — the user's first
reaction was to wonder if they needed a different account, when it was
actually just a client-side bug (real filesystem permissions were never
involved).
**Rule:** relative-path shorthands are fine for the *first* lookup, but don't
build ongoing navigation state machine off them — resolve to a real absolute
path once (here: `sftp.canonicalize(".")`) and do all subsequent join/parent
logic in absolute-path terms, which have a natural, real ceiling (`/`) instead
of an artificial one you have to invent and get wrong.
