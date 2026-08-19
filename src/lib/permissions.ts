// A POSIX file mode is twelve bits: three special bits (setuid, setgid, sticky)
// above the owner/group/other read-write-execute triads. Everything here reads and
// writes that single number, so the permissions dialog's octal field and checkbox
// grid stay two views of one value instead of two states that can disagree.

export const SET_UID = 0o4000;
export const SET_GID = 0o2000;
export const STICKY = 0o1000;

/** Every bit a mode may legitimately carry — the rest are file *type* bits. */
export const MODE_BITS = 0o7777;

export type Who = "owner" | "group" | "other";
export type What = "read" | "write" | "exec";

const WHO_SHIFT: Record<Who, number> = { owner: 6, group: 3, other: 0 };
const WHAT_BIT: Record<What, number> = { read: 4, write: 2, exec: 1 };

export function hasBit(mode: number, who: Who, what: What): boolean {
  return (mode & (WHAT_BIT[what] << WHO_SHIFT[who])) !== 0;
}

export function withBit(mode: number, who: Who, what: What, on: boolean): number {
  const bit = WHAT_BIT[what] << WHO_SHIFT[who];
  return on ? mode | bit : mode & ~bit;
}

export function hasSpecial(mode: number, bit: number): boolean {
  return (mode & bit) !== 0;
}

export function withSpecial(mode: number, bit: number, on: boolean): number {
  return on ? mode | bit : mode & ~bit;
}

// Three digits for a plain mode and four once a special bit is set — the shape
// people are used to typing and reading (`755`, but `1777` for a sticky /tmp).
export function formatOctal(mode: number): string {
  const masked = mode & MODE_BITS;
  return masked.toString(8).padStart(masked & ~0o777 ? 4 : 3, "0");
}

// Parses what the octal field currently holds. Returns null for anything that
// isn't one to four octal digits, which is also every half-typed value.
export function parseOctal(text: string): number | null {
  if (!/^[0-7]{1,4}$/.test(text)) return null;
  return parseInt(text, 8);
}

// The `ls -l` rendering, special bits folded into the execute slot exactly as ls
// shows them: lowercase `s`/`t` when the execute bit is set too, uppercase when
// it isn't (an `S` means the special bit will do nothing, which is worth seeing).
export function formatSymbolic(mode: number, isDir: boolean, isSymlink: boolean): string {
  const triad = (who: Who, special: number, specialChar: string) => {
    const exec = hasBit(mode, who, "exec");
    const overloaded = hasSpecial(mode, special);
    return (
      (hasBit(mode, who, "read") ? "r" : "-") +
      (hasBit(mode, who, "write") ? "w" : "-") +
      (overloaded ? (exec ? specialChar : specialChar.toUpperCase()) : exec ? "x" : "-")
    );
  };

  return (
    (isSymlink ? "l" : isDir ? "d" : "-") +
    triad("owner", SET_UID, "s") +
    triad("group", SET_GID, "s") +
    triad("other", STICKY, "t")
  );
}

// The four modes that cover almost every reason to open this dialog at all.
export const MODE_PRESETS: { mode: number; hint: string }[] = [
  { mode: 0o644, hint: "File — owner writes, everyone reads" },
  { mode: 0o755, hint: "Script or folder — owner writes, everyone reads and runs" },
  { mode: 0o600, hint: "Private file — owner only" },
  { mode: 0o700, hint: "Private folder — owner only" },
];
