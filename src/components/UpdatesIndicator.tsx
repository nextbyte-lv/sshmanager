import { useCallback, useEffect, useRef, useState } from "react";
import { Package, PackageCheck, PackageX, Power, RefreshCw, TerminalSquare } from "lucide-react";

import { ConfirmDialog } from "@/components/ConfirmDialog";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { formatAge, formatDuration } from "@/lib/monitor";
import { updatesStatus } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import type { HostAction, UpdateStatus } from "@/types/updates";

// Opening the menu re-checks when the last answer is older than this, so the
// status is current again after an upgrade run in the terminal, without polling.
const RECHECK_AFTER_MS = 30_000;

// Past this, the pending count is computed against lists so old it says little
// about what the mirrors actually offer now.
const STALE_LISTS_SECONDS = 7 * 86400;

// "Last checked" turns amber past this, as a hint that the numbers are old.
const STALE_CHECK_MS = 60 * 60 * 1000;

// How many package names to list before summarising the rest.
const LISTED_PACKAGES = 8;

type Level = "ok" | "pending" | "urgent";

function levelOf(status: UpdateStatus): Level {
  if (status.pending_security > 0 || status.reboot_required) return "urgent";
  const listsAge = status.lists_refreshed_at === null ? Infinity : status.checked_at - status.lists_refreshed_at;
  if (status.pending.length > 0 || listsAge > STALE_LISTS_SECONDS) return "pending";
  return "ok";
}

const LEVEL_CLASS: Record<Level, string> = {
  ok: "text-emerald-500",
  pending: "text-amber-500",
  urgent: "text-red-500",
};

/** What the terminal will run, for the menu (the backend builds the real line). */
function upgradeLabel(status: UpdateStatus | null): string {
  return status?.is_root ? "apt update && apt upgrade" : "sudo: apt update && apt upgrade";
}

interface UpdatesIndicatorProps {
  sessionId: string | null;
  manageUpdates: boolean;
  onRun: (action: HostAction, asRoot: boolean) => void;
}

export function UpdatesIndicator({ sessionId, manageUpdates, onRun }: UpdatesIndicatorProps) {
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const [confirmReboot, setConfirmReboot] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const fetchedAt = useRef(0);
  // Each request's own number, so an answer for a session that has since been
  // replaced (reconnect, pane reused) cannot overwrite the current one.
  const request = useRef(0);

  const refresh = useCallback(() => {
    if (!sessionId) return;
    const mine = ++request.current;
    setLoading(true);
    updatesStatus(sessionId)
      .then((next) => {
        if (mine !== request.current) return;
        setStatus(next);
        setError(null);
      })
      .catch((err) => {
        if (mine === request.current) setError(String(err));
      })
      .finally(() => {
        if (mine !== request.current) return;
        fetchedAt.current = Date.now();
        setLoading(false);
      });
  }, [sessionId]);

  useEffect(() => {
    setStatus(null);
    setError(null);
    refresh();
  }, [refresh]);

  // Ticks only while the menu is open, for the "12s ago" beside Last checked.
  const [menuOpen, setMenuOpen] = useState(false);
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!menuOpen) return;
    setNow(Date.now());
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [menuOpen]);

  // Nothing until there is something to say, and nothing at all on non-apt hosts.
  if (!sessionId || (!status && !error) || (status && !status.supported)) return null;

  const level = status ? levelOf(status) : null;
  const Icon = !status ? Package : level === "ok" ? PackageCheck : level === "urgent" ? PackageX : Package;
  const pendingCount = status?.pending.length ?? 0;
  const ago = (at: number | null) => (status && at !== null ? `${formatAge(at, status.checked_at)} ago` : "unknown");

  const securityFirst = status
    ? [...status.pending].sort((a, b) => Number(b.security) - Number(a.security))
    : [];

  const asRoot = status?.is_root ?? false;

  return (
    <>
      <DropdownMenu
        onOpenChange={(open) => {
          setMenuOpen(open);
          if (open && !loading && Date.now() - fetchedAt.current > RECHECK_AFTER_MS) refresh();
        }}
      >
        <DropdownMenuTrigger
          render={
            <Button
              size="xs"
              variant="ghost"
              title={status ? summary(status) : `Update check failed: ${error}`}
              className={cn(level ? LEVEL_CLASS[level] : "text-muted-foreground")}
            />
          }
        >
          <Icon />
          {pendingCount > 0 && <span className="tabular-nums">{pendingCount}</span>}
        </DropdownMenuTrigger>

        <DropdownMenuContent className="w-80">
          {/* Base UI throws (and takes the whole app down) if a group label is
              rendered outside a group — see tasks/lessons.md. */}
          <DropdownMenuGroup>
            <DropdownMenuLabel>{status?.distro ?? "Package updates"}</DropdownMenuLabel>

            {/* `grid-cols-1` is minmax(0,1fr): without it the column grows to its
                widest row's content, and one long value pushes every right-aligned
                value out of the popup. */}
            <div className="grid grid-cols-1 gap-1 px-1.5 pb-1.5 text-xs">
              {error && <p className="text-destructive">{error}</p>}
              {status && (
                <>
                  {/* This PC's clock on purpose: it is when *the app* got the answer,
                      and a host whose clock is off must not show "-3s ago". */}
                  <Row
                    label="Last checked"
                    value={
                      loading
                        ? "checking…"
                        : `${formatDateTime(Math.floor(fetchedAt.current / 1000))} (${since(now - fetchedAt.current)})`
                    }
                    // Blue, not green: green/amber/red already mean up to date /
                    // pending / urgent in this menu, and this is not a status.
                    className={
                      !loading && now - fetchedAt.current > STALE_CHECK_MS ? "text-amber-500" : "text-sky-400"
                    }
                  />
                  <Row
                    label="Package lists refreshed"
                    value={ago(status.lists_refreshed_at)}
                    title={formatDateTime(status.lists_refreshed_at)}
                  />
                  <Row
                    label="Last package change"
                    value={ago(status.packages_changed_at)}
                    title={formatDateTime(status.packages_changed_at)}
                  />
                  <Row
                    label="Pending upgrades"
                    value={
                      pendingCount === 0
                        ? "none"
                        : `${pendingCount}${status.pending_security ? ` (${status.pending_security} security)` : ""}`
                    }
                    className={level ? LEVEL_CLASS[level] : undefined}
                  />
                  {status.reboot_required && (
                    <div className="text-red-500">
                      <Row label="Reboot required" value="yes" className="text-red-500" />
                      {/* The reason gets its own wrapping line: kernel versions are
                          long enough to never fit beside the label. */}
                      {(status.newer_kernel || status.reboot_packages.length > 0) && (
                        <p className="mt-0.5 font-mono text-[0.7rem] break-all">
                          {status.newer_kernel
                            ? `kernel ${status.newer_kernel} installed, running ${status.running_kernel}`
                            : status.reboot_packages.join(", ")}
                        </p>
                      )}
                    </div>
                  )}
                  <Row
                    label="Unattended upgrades"
                    value={
                      !status.unattended_installed
                        ? "not installed"
                        : !status.unattended_enabled
                          ? "installed, off"
                          : "on"
                    }
                  />
                  {status.unattended_last_run !== null && (
                    <Row
                      label="Last automatic run"
                      value={
                        status.unattended_last_result
                          ? `${ago(status.unattended_last_run)} · ${status.unattended_last_result}`
                          : ago(status.unattended_last_run)
                      }
                      title={
                        status.unattended_last_result
                          ? formatDateTime(status.unattended_last_run)
                          : `${formatDateTime(status.unattended_last_run)} — the server has rebooted since, so how that run ended is no longer known`
                      }
                      // Red only for a real failure; an unknown result is not one.
                      className={
                        status.unattended_last_result && status.unattended_last_result !== "success"
                          ? "text-red-500"
                          : undefined
                      }
                    />
                  )}
                  {status.unattended_next_run !== null && (
                    <Row
                      label="Next automatic run"
                      value={
                        status.unattended_next_run > status.checked_at
                          ? `in ${formatDuration(status.unattended_next_run - status.checked_at)}`
                          : "due now"
                      }
                      title={formatDateTime(status.unattended_next_run)}
                    />
                  )}

                  {pendingCount > 0 && (
                    <ul className="mt-1 grid gap-0.5 font-mono text-[0.7rem] text-muted-foreground">
                      {securityFirst.slice(0, LISTED_PACKAGES).map((p) => (
                        <li key={p.name} className="truncate" title={`${p.from_version ?? "?"} → ${p.version}`}>
                          <span className={p.security ? "text-red-500" : undefined}>{p.name}</span> {p.version}
                        </li>
                      ))}
                      {pendingCount > LISTED_PACKAGES && <li>… and {pendingCount - LISTED_PACKAGES} more</li>}
                    </ul>
                  )}
                </>
              )}
            </div>
          </DropdownMenuGroup>

          <DropdownMenuSeparator />
          <DropdownMenuItem onClick={refresh} disabled={loading}>
            <RefreshCw className={cn(loading && "animate-spin")} />
            Check again
          </DropdownMenuItem>
          {manageUpdates && (
            <DropdownMenuItem onClick={() => onRun("upgrade", asRoot)}>
              <TerminalSquare />
              <span className="flex flex-col">
                Run updates in terminal
                <span className="font-mono text-[0.7rem] text-muted-foreground">{upgradeLabel(status)}</span>
              </span>
            </DropdownMenuItem>
          )}
          {manageUpdates && (
            <DropdownMenuItem
              variant={status?.reboot_required ? "destructive" : "default"}
              onClick={() => setConfirmReboot(true)}
            >
              <Power />
              Reboot server…
            </DropdownMenuItem>
          )}
        </DropdownMenuContent>
      </DropdownMenu>

      <ConfirmDialog
        open={confirmReboot}
        onOpenChange={setConfirmReboot}
        title="Reboot this server?"
        description={
          <>
            Every session on the host is cut off, including this one. The pane reconnects
            on its own once the server is back, if that happens within about half a minute;
            otherwise use Reconnect.
            {status && status.pending.length > 0 && (
              <span className="mt-2 block text-amber-500">
                {status.pending.length} upgrades are still pending — you may want to run updates
                first.
              </span>
            )}
          </>
        }
        confirmLabel="Reboot"
        destructive
        onConfirm={() => onRun("reboot", asRoot)}
      />
    </>
  );
}

function summary(status: UpdateStatus): string {
  const parts = [
    status.pending.length === 0
      ? "Up to date"
      : `${status.pending.length} upgrades pending${status.pending_security ? `, ${status.pending_security} security` : ""}`,
  ];
  if (status.reboot_required) parts.push("reboot required");
  return parts.join(" · ");
}

/**
 * `dd.mm.yyyy hh:mm`, 24-hour, in this PC's time zone. Built by hand rather than
 * with `toLocaleString("lv-LV")`, whose exact output (trailing dot, separators)
 * varies between WebView2 versions.
 */
function formatDateTime(epochSeconds: number | null): string {
  if (epochSeconds === null || !epochSeconds) return "unknown";
  const d = new Date(epochSeconds * 1000);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(d.getDate())}.${pad(d.getMonth() + 1)}.${d.getFullYear()} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

function since(ms: number): string {
  const seconds = Math.max(0, Math.floor(ms / 1000));
  if (seconds < 5) return "just now";
  if (seconds < 60) return `${seconds}s ago`;
  return `${formatDuration(seconds)} ago`;
}

function Row({
  label,
  value,
  title,
  className,
}: {
  label: string;
  value: string;
  /** Hover text; defaults to the value, for when it is truncated. */
  title?: string;
  className?: string;
}) {
  return (
    <div className="flex min-w-0 justify-between gap-3">
      <span className="shrink-0 text-muted-foreground">{label}</span>
      <span className={cn("min-w-0 truncate text-right", className)} title={title ?? value}>
        {value}
      </span>
    </div>
  );
}
