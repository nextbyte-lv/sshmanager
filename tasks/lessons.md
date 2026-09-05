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

## `npx <tool>` for a tool the project does not depend on rewrites the manifest

Running `npx prettier --write src/...` to tidy formatting did three unwanted
things at once. Prettier is not a dependency here, so npx installed it, and that
install resolved every dependency range afresh and wrote the results back:
`package.json` gained bumped carets (`lucide-react` `^1.25.0` -> `^1.33.0`) and
`package-lock.json` picked up ~60 transitive version changes. `node_modules` was
refreshed to match, so the lockfile and the installed tree no longer agreed once
the manifest edits were reverted (`npm ci` puts them back in step).

The formatting itself was the third problem: with no `.prettierrc` in the repo,
prettier applied its own defaults and reformatted whole files — 1400 changed
lines in `SftpPanel.tsx` alone — burying the actual change.

Root cause: `npx` on a missing package is an install, not a sandboxed run, and
npm treats any install as licence to re-resolve the manifest.

**This happens even when the package *is* a devDependency and already
installed.** `npx tsc --noEmit` — the typecheck command this project's own
CLAUDE.md used to document — rewrote `package.json` (bumped carets on
`@base-ui/react`, `lucide-react`, `shadcn`, `vite`, the `@types/*`) and ~700
lines of `package-lock.json`, with `typescript@7.0.2` already present under
`node_modules/.bin/tsc` and satisfying the `~7.0.2` range. Run project binaries
directly instead: `./node_modules/.bin/tsc --noEmit`. If a manifest rewrite has
already happened, `git checkout -- package.json package-lock.json` then `npm ci`
— the checkout alone leaves the installed tree out of step with the lockfile. There is no
formatter configured for this project; match the surrounding file's style by hand
instead (4-space indent under `src/components/`, 2-space under `src/lib/`).

## One error state shared by a refresh and by user actions loses the message

The SFTP panel had a single `error` slot. `refresh()` opened with
`setError(null)`, and `uploadPaths()` ended with `refresh()` — so every upload
error the panel had just recorded (the `file_error` events from the channel, and
the rejection of `sftp_upload` itself) was cleared microseconds after being set.
A failed upload was indistinguishable from a click that did nothing: no
progress, no error, nothing. The picker calls made it worse — `openFileDialog`
and `saveFileDialog` were awaited outside any `try`, so a dialog that rejects
became an unhandled rejection in a click handler, which is silent by
construction.

**Rule:** an error slot belongs to exactly one producer. State cleared by a
reload (`listError`) and state describing what the user just asked for
(`actionError`) are two different lifetimes; sharing one variable means whichever
runs last wins, and the reload always runs last. And every `await` in a click
handler needs a `catch` that reaches the screen — in a desktop webview there is
no console anyone is watching.

## `sh -c '<script>'` is not actually safe from the login shell — send it on stdin

The remote monitor needs a multi-line POSIX script with an awk program in it. The
lesson above says `exec::run` goes through the account's *login* shell, so shell
operators are unsafe there; the obvious dodge is to hand the whole script to
`sh -c` as one single-quoted word, on the theory that a quoted word is inert
everywhere. It isn't — in exactly the two shells that lesson names. **fish**
honours `\'` and `\\` *inside* single quotes, and **csh/tcsh** performs history
expansion on `!` inside them. An awk program parsing `/proc` is full of both
(`\t`, `[0-9]`, `!seen[x]`), so the "safe" form silently imposes "no backslashes,
no `!`, no newlines" on the script.

**Rule:** send the script as channel **stdin** to a bare `sh`
(`exec::run(&ssh, "sh", Some(script))`) — the `ssh host sh < script.sh` idiom. The
login shell then only ever sees the two-character word `sh`, and the script bytes
are parsed by a real POSIX shell, so they may contain anything. `ssh/exec.rs`
already writes stdin then EOFs before collecting the exit status, so this needs no
plumbing. Two consequences worth knowing: the script can live as a reviewable
`.sh` file (`include_str!`) instead of an escaped Rust literal, and stdin is now
spent, so that call can never also carry a sudo password — fine for sampling,
which must not use sudo anyway.

## `awk file1 file2 …` over `/proc/[0-9]*/stat` truncates the list silently

Reading every process means globbing hundreds of `/proc/<pid>/stat` files, and a
pid exiting between glob expansion and the read is normal, not exceptional. If
those files are passed to `awk` as *operands*, the first failed open is **fatal**
in gawk, mawk and busybox awk: awk abandons the remaining operands and exits 2.
Stdout still looks perfectly well-formed — it is just missing an arbitrary tail of
the process list, and on a busy host that happens most polls.

**Rule:** `cat /proc/[0-9]*/stat 2>/dev/null | awk '…'`, never
`awk '…' /proc/[0-9]*/stat`. `cat` reports the missing file on stderr and carries
on to the next operand. Related, same collector: don't gate the sample on the exit
status either — `cat` exits nonzero whenever any pid vanished, i.e. almost always.
Emit an `@@end` sentinel as the script's last line and treat *that* as the
completeness check; it also catches a `ForceCommand`/`nologin` shell having run
something else entirely, which an exit status cannot.

## `/proc/<pid>/stat` field 2 defeats every naive field index

`comm` is parenthesised and may contain spaces **and** parentheses
(`(evil ) name)`), so `$14`-style indexing shifts by however many spaces are in
the name. Indexing backwards from `NF` is wrong too — the field count grew across
kernel versions (44 → 52). Split on the **last** `)` (`match($0, /\)[^)]*$/)`) and
index the remainder: state 1, ppid 2, utime 12, stime 13, num_threads 18,
starttime 20, rss 22. The failure is invisible: `nice` (19) and `num_threads` (20)
are both plausible small integers.

Other members of the same family, every one of which produces a *plausible*
number rather than an error:
- **RSS in `stat` is in pages, not kB** — scale by `getconf PAGESIZE`. It is 65536
  on 64K-page arm64 and ppc64le, so a hardcoded 4096 is a silent 16x error.
- **`/proc/net/dev` has no space after the `:`** once a counter is wide enough
  (`eth0:1234567890123`), so whitespace splitting shifts every field. Split on the
  first `:`.
- **`user` already includes `guest`** in `/proc/stat` (and `nice` includes
  `guest_nice`), so summing all ten fields inflates the denominator on a KVM host.
- **`df`'s own `Use%` is `used/(used+avail)`**, not `used/size` — the root-reserved
  5% is in neither, so the obvious calculation disagrees with `df -h` on every
  ext4 volume.
- **`ps -o user=` truncates to 8 characters** with a `+` suffix; use `user:32=`.
  And `argv` may contain a newline, which shifts every following row and pairs one
  process with another's command line — accept a `ps` line as a row only if it
  starts with a pid, and append anything else to the previous row.
- **Per-core `/proc/stat` lines cover only *online* CPUs**, so key them by their
  `cpuN` label; by array position, one core going offline shifts every later core.
- **`df` and `/proc/mounts` are mostly noise**: a WSL Debian mounts twenty tmpfs
  filesystems and a stock Ubuntu has a squashfs per snap, all 100% full by
  construction. Filter by fs type or the real volumes never make it onto the card.

**Rule:** for anything derived from `/proc`, write the parser as a pure function
and test it against two *captured real samples*, not against the UI. Every trap
above is caught by a fixture in seconds and by no amount of looking at a screen.
`src-tauri/src/ssh/testdata/` holds a consecutive pair taken from a live host with
a known `yes > /dev/null` running, which pins the per-process CPU maths to a real,
verifiable 100% of one core.

## Counter deltas need the remote clock, `checked_sub`, and a `(pid, starttime)` key

Three separate ways a rate goes quietly wrong:
- Measuring elapsed time with the *local* poll interval is off by 10–30% once SSH
  latency and scheduler jitter are involved, and that error scales every
  byte-per-second figure. Use the delta of the remote `/proc/uptime` instead.
- A counter can go **backwards** — a reboot, a 32-bit wrap on a busy link, an
  interface bounce, or our own reconnect. `checked_sub` and discard the whole
  sample; clamping to zero reports a saturated link as "0 B/s", which reads as
  nothing happening.
- Keying the previous sample by `pid` alone diffs a *new* process against a dead
  one's counters when the pid is recycled, giving an absurd spike. Key by
  `(pid, starttime)`, as htop does. The same `starttime` is what a kill should
  re-check before signalling, or pid reuse between rendering a row and clicking it
  kills something unrelated.

Cache invalidation belongs with this: a per-session cache of a previous sample has
to be dropped everywhere `state.sftp` is (`close_session`, and **both** reconnect
branches in `ssh/pty.rs`), or the first sample after a host reboot subtracts large
old counters from small new ones.

Concurrency belongs with it too. Two overlapping polls sharing one "previous
sample" slot each end up diffing against the other's sample, halving the measured
interval and making every rate wrong for as long as it lasts. The slot needs a
`tokio::sync::Mutex` (async — it is held across the collection await), cloned out
from under the `std::Mutex<HashMap<..>>` before awaiting, exactly as `get_sftp`
does.

## `exec::run` has no timeout — a hung NFS mount freezes the caller forever

`exec::run` loops `channel.wait()` until the channel ends, with no deadline. A
`df` blocked on a dead NFS/CIFS mount therefore leaves the future pending
indefinitely, and with a chained-`setTimeout` poller on the frontend the panel
simply stops updating with nothing on screen to say why. Wrap calls that touch the
remote filesystem in `tokio::time::timeout` at the call site. GNU `df -l` skips
remote mounts and helps, but busybox has no `-l`, so it needs the usual
probe-and-fallback and the timeout is still the backstop. `sftp_dir_sizes`' `du`
has the same latent hazard.

## `shadcn add` wired new components to an npm package called `cn`

`./node_modules/.bin/shadcn add table progress` (the local binary — `npx shadcn`
rewrites the manifest, see above) created both files with
`import { cn } from "cn"` and added a real dependency, `"cn": "^0.2.5"`, to
`package.json` — instead of the `@/lib/utils` alias every other component in
`src/components/ui/` uses and that `components.json` itself declares. It builds
and runs, so nothing complains; the project just quietly grows a second `cn`.

**Rule:** after any `shadcn add`, diff `package.json` and grep the generated files
for imports that don't match the project's own conventions, before writing any
code against them. Recovery here was `git checkout -- package.json
package-lock.json` plus removing that one package from `node_modules`; a full
`npm ci` is the answer if more than one package landed.

## shadcn's `Table` brings its own scroll container, which breaks a sticky header

`Table` renders `<div data-slot="table-container" class="relative w-full
overflow-x-auto">` around the `<table>`. A box with **one** axis set to something
other than `visible` computes the other axis to `auto` as well, so that wrapper is
a scroll container in *both* directions — and therefore the nearest scrolling
ancestor of a `sticky top-0` `<thead>`. Having no bounded height it never scrolls
vertically, so the header sticks to a container that cannot move, and scrolling
the div *outside* it slides the header away. Nothing errors.

**Rule:** when putting a shadcn `Table` in a scrolling panel, bound the wrapper
rather than an ancestor — `overflow-hidden` on the outer flex child plus
`[&>[data-slot=table-container]]:h-full` — so the vertical scroll happens in the
same box the sticky header is measured against. Add `table-fixed` as well if the
column width classes and `truncate` are meant to do anything: without it they are
advisory, and one long cell stretches the table instead of being clipped.

## `core.autocrlf=true` will corrupt a shell script shipped to a remote host

This repo had no `.gitattributes` and the machine has `core.autocrlf=true`, so any
`.sh` committed here is checked out with CRLF. `src-tauri/src/ssh/monitor.sh` is
`include_str!`ed and piped verbatim to a remote POSIX `sh`, where every line would
arrive with a trailing `\r`: `export: not found`, and `@@` section markers that
never match. The failure surfaces on a remote host, a long way from its cause, and
only for whoever cloned the repo — never for the machine that wrote the file.

**Rule:** any file whose bytes are shipped somewhere else verbatim needs an
explicit `.gitattributes` entry (`*.sh text eol=lf`, and the same for captured
test fixtures). Belt and braces, normalise at the boundary too — the collector
does `include_str!(…).replace("\r\n", "\n")` so a stray editor cannot reintroduce
it.

## A GUI-less way to test a remote-host feature: WSL is a real Linux host

The monitor's collector script and every parser in `ssh/monitor.rs` were verified
without a server and without launching the app, by piping `monitor.sh` into
`wsl.exe -d Debian -- sh` and reading what came back. That is a genuine Linux
`/proc`, `ps`, `df` and `ss`, and it caught three real defects that no amount of
reading would have: a stderr redirect that fired before `tr` ran, twenty tmpfs
mounts crowding out the real volumes, and `ps` column padding the row parser
mishandled. Capturing two consecutive samples two seconds apart, with a known
`yes > /dev/null` in between, then became the regression fixture.

**Rule:** before reaching for the app and a live server to test something that
mostly consists of parsing a remote system's output, check whether the output can
be produced locally. Two traps when doing it from Git Bash: MSYS rewrites
unix-looking arguments into Windows paths (`wsl -- cat /tmp/s1` becomes
`C:/…/Temp/s1`), and a background process started in one `wsl.exe` invocation does
not survive into the next — do the whole capture inside a single script piped to
one invocation.

## In a live-updating panel, a value that changes width shoves everything near it

The host monitor's disk rows read `r 33.6 KB/s  w —`, and every field was sized by
its own text. Devices are idle most of the time, so on each refresh a rate flipped
to a dash and back, each change resizing the box and sliding the neighbouring
labels sideways. Worse, the whole disk block was rendered only when *something*
was busy (`disks.some(d => read + write > 0)`), so it appeared and vanished and
took the rest of the card up and down with it. Individually both looked like
reasonable code; together they made a panel that will not sit still.

**Rule:** in anything that re-renders on a timer, a changing value needs a slot
that does not depend on what is in it, and a row that can go quiet must still be
rendered — a dash holds its place, an absent row does not. For monospace figures
`w-[9ch]` sizes the slot by the longest output the *formatter* can produce
(`1024 KB/s`) and scales with the font, which is more honest than a pixel width
guessed from one sample. Work out that longest string from the formatter rather
than from what is on screen: `formatBytes` caps at 7 characters because it drops
the decimal above 100, which is not obvious from looking at `6.1 GB`.

Worth checking the whole surface once one instance shows up: the same defect was
in the CPU `usr/sys/io/steal` line and the memory `cache/buffers` line, both of
which had shipped in the same panel.
