// Mirrors `UpdateStatus` in `src-tauri/src/ssh/updates.rs`. Timestamps are epoch
// seconds on the *host's* clock; compare them with `checked_at`, never Date.now().

/** Mirrors `HostAction` in `src-tauri/src/commands/updates.rs`. */
export type HostAction = "upgrade" | "reboot";

export interface PendingPackage {
  name: string;
  version: string;
  from_version: string | null;
  security: boolean;
}

export interface UpdateStatus {
  /** False on non-apt hosts: show nothing at all. */
  supported: boolean;
  distro: string | null;
  is_root: boolean;
  checked_at: number;
  lists_refreshed_at: number | null;
  packages_changed_at: number | null;
  pending: PendingPackage[];
  pending_security: number;
  reboot_required: boolean;
  reboot_packages: string[];
  running_kernel: string | null;
  newer_kernel: string | null;
  unattended_installed: boolean;
  unattended_enabled: boolean;
  /** Known across reboots. */
  unattended_last_run: number | null;
  /** Only known when that run happened since the last boot. */
  unattended_last_result: string | null;
  unattended_next_run: number | null;
}
