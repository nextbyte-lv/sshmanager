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

## A file watcher hears its own program's writes — a downloader that also uploads echoes
The SFTP "open remote file in local editor" flow downloads to a local cache
dir, watches that dir with `notify`, and re-uploads on change. It worked on the
first open of a file and then started failing with
`sftp error: Permission denied: Permission denied` on later opens. Nothing about
permissions had changed on the server, and the frontend was innocent.
Root cause: `watched_dirs` arms the directory watch only once, *after* the first
download. So on the first open the download's own write happens before the
watch exists and goes unseen — but on every later open of a file in that same
directory the watch is already live, the download's write is reported as a
change, and the "user edited it" path fires and uploads the file straight back.
For a remote file the SSH user can't write (a root-owned `.sh`), merely *opening*
it then produced a write error; for writable files it was a silent pointless
round-trip that bumped the remote mtime.
**Rule:** whenever the same component both writes a file and watches it, the
watcher will hear the component's own writes — event-arrival order relative to
watch registration is not a defence, it just makes the bug intermittent. Gate on
observed content state, not on event occurrence: record a stamp (mtime + size)
of what was last synced and ignore any event whose stamp still matches
(`WatchedFile.synced` in `state.rs`). Attribute-only touches from editors get
filtered by the same check for free.
**Also:** `russh_sftp` renders a status packet as `"<code>: <message>"` and
servers usually put the code's own text in the message, so raw errors read
`Permission denied: Permission denied` and name neither the file nor the
operation. Wrap remote failures with the path and the verb
(`SshError::RemoteRead`/`RemoteWrite`) — an error the user can act on has to say
*which* file and *what* was attempted.

## SFTP has no sudo — privileged writes need a second, non-SFTP channel
Editing `/usr/local/bin/scheduled-shutdown.sh` through the SFTP panel failed with
`Permission denied` on save and there is no client-side fix for it: the SFTP
subsystem is a process running as the *login user*, and the protocol has no
concept of privilege escalation anywhere in it — no flag on `SSH_FXP_OPEN`, no
per-request identity. `sftp.create()` is just `open(O_WRONLY|O_TRUNC)` under a
non-root uid, so the kernel refuses it and that is the end of the story on that
channel.
**Rule:** when SFTP can't express an operation, open a *separate* exec channel on
the same `Arc<Handle>` (`ssh/exec.rs`) rather than looking for a stronger SFTP
call. The write becomes: stage the bytes at a path the login user *can* write
(`/tmp/.sshmanager-<uuid>`), then `sudo cp` it onto the target.
Details that matter:
- Use `cp`, not `mv`. `cp` onto an existing path writes through to the target's
  inode and leaves owner and mode alone; `mv` replaces the file with one owned by
  root at the temp file's mode, silently re-owning whatever you edited.
- Never put a password on the command line — it is visible to every user on the
  box via `ps`. Send it on the channel's stdin (`sudo -S -p ''`), and with no
  password to offer use `sudo -n` so it fails fast instead of blocking forever on
  a prompt nothing will ever answer.
- Remote paths come from the server's own listing and can contain quotes, `$`,
  and spaces. Anything interpolated into an exec command needs POSIX
  single-quoting (`shell_quote`, covered by the one unit test in the repo).
- A key-based connection has only a key *passphrase* stored, which is not the
  login password: never hand it to sudo.

## A recursive chmod that applies as it walks can lock itself out
SFTP has no recursive `setstat`, so "apply this mode to everything inside" has to
be a client-side walk. The obvious shape — visit a directory, chmod it, then list
it and recurse — breaks on exactly the modes people ask for: `0644` or `0600` on a
directory clears its execute bit, and execute on a directory *is* the right to
traverse it, so the very next `read_dir` gets `Permission denied` from a
permission the client itself just removed. Half the tree is re-moded and the rest
is now unreachable through the panel.
**Rule:** finish the walk before mutating anything, then apply to files first and
directories deepest-first (`set_mode_recursive` in `ssh/sftp.rs`). Same shape as
`remove_dir_all` in that file, for the same underlying reason: a tree operation
whose per-entry action can invalidate the traversal must separate discovery from
mutation. Also skip symlinks — `setstat` follows them, so chmod-ing a link
re-modes its target, which may be outside the tree entirely (`chmod -R` skips
them for this reason).

## A blurry Windows icon means a missing size in the .ico, not a low-res source
The desktop icon looked soft while the artwork itself was fine at every size we
had. `icon.ico` shipped only 16, 32, 128 and 256 px frames — but the Windows
desktop at 100% scaling draws "Medium icons" at exactly **48 px**, and Explorer's
"Large icons" at 96. With no 48 px frame the shell picks a neighbouring one and
rescales it itself, using a fast filter, so the icon arrives smeared. Nothing
about the source art or the build was wrong; the size simply wasn't in the file.
Worth knowing that `tauri icon` does *not* solve this on its own: it emits
16/24/32/48/64/256, which fixes 48 but still leaves 96 and 128 to be rescaled,
and it silently overwrites `icon.ico` — so re-running it will drop any extra
frames added by hand.
**Rule:** an .ico is a container of independent renders, so give it a frame at
every size the shell asks for — 16, 20, 24, 32, 40, 48, 64, 96, 128, 256 (the
20/40 entries cover 125% scaling, 24/48 cover 150%). Verify by *decoding* the
file rather than trusting the generator: `System.Windows.Media.Imaging.IconBitmapDecoder`
lists every frame and its real dimensions. Two traps when checking or building
one by hand in PowerShell:
- `System.Drawing.Icon` cannot decode PNG-compressed frames (which is what modern
  generators emit) and throws "range extends past the end of the array" on a
  perfectly valid file — it is not evidence of corruption. Use the WPF decoder.
- `Win32 PrivateExtractIcons` always returns the size you asked for, rescaling
  silently, so it proves the file *loads* but can never show you a missing size.
  To see the difference, extract the frame and look at it.
- A PowerShell array slice (`$bytes[$a..$b]`) is `Object[]`, and
  `BinaryWriter.Write(Object[])` writes a *single byte* instead of failing. Cast
  to `[byte[]]` or the output is silently truncated while the length field still
  claims the full size.

## Editing an icon does not rebuild the exe's icon — cargo never sees the change
After replacing `icons/icon.ico`, `tauri build` recompiled the crate, reported
success, and produced an exe still carrying the *old* four-frame icon. The icon
is baked into the binary as `RT_ICON`/`RT_GROUP_ICON` resources by
`tauri_build::build()` in `build.rs`, and that build script does not emit a
`cargo:rerun-if-changed` for the icon files. Cargo therefore considers the build
script fresh, skips it, reuses the previously generated resource, and relinks —
so the compile is real but the icon is stale, and nothing in the output hints at
it. Touching `src-tauri/build.rs` (mtime only) invalidates the build-script
fingerprint and regenerates the resource; `cargo clean -p sshmanager` also works.
**Rule:** after changing anything under `src-tauri/icons/`, touch `build.rs`
before building, and confirm against the *exe* rather than the source file —
enumerate its `RT_ICON` resources (`LoadLibraryEx` + `EnumResourceNames`) and
check the frame list is the one you expect. "Compiling…/Finished" says nothing
about resources. The same blind spot applies to any asset a build script consumes
without declaring it.

## Moving the project directory breaks the build — delete `src-tauri/target/`
After moving the repo from `D:\Work\Git\Arccuks\SshManager` to
`D:\Work\Git\NextByte\SshManager`, `npm run tauri build` failed pointing at a
generated permission file under the *old* root
(`...\Arccuks\...\build\tauri-<hash>\out\permissions\app\autogenerated\commands\app_hide.toml`).
Nothing in the repo hardcodes a path — the project source is fully portable.
Cargo's build cache is simply not relocatable: `tauri_build::build()` writes
permission `.toml`s into `OUT_DIR` and records their **absolute** paths into
`out/tauri-core-*-permission-files`, and every `.d` dep-info file in
`target/*/deps/` is absolute too. On rebuild Cargo replays paths that no longer
exist. There is no setting that makes these relative — absolute paths in
`target/` are by design.
**Rule:** after moving or renaming any parent directory of this repo, delete
`src-tauri/target/` (`cargo clean`) and `node_modules/.vite`. Both are
gitignored, disposable build output; the only cost is one full rebuild. Don't
investigate — the stale-cache-after-move symptom always has the same one-line
fix, and grepping the cache to surgically patch it wastes far more time than the
rebuild it tries to avoid.
