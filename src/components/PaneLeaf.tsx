import { useRef, useState } from "react";
import { MosaicWindow, type MosaicPath, type MosaicDirection } from "react-mosaic-component";
import { ChevronsDown, ChevronsRight } from "lucide-react";

import { MonitorPanel } from "@/components/MonitorPanel";
import { PaneToolbar } from "@/components/PaneToolbar";
import { SftpPanel } from "@/components/SftpPanel";
import { TerminalPane } from "@/components/TerminalPane";
import { UpdatesIndicator } from "@/components/UpdatesIndicator";
import { Button } from "@/components/ui/button";
import { useResizablePanel } from "@/hooks/useResizablePanel";
import { updatesRun } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import type { ConnectionProfile } from "@/types/connection";
import type { HostAction } from "@/types/updates";

interface PaneLeafProps {
  path: MosaicPath;
  connection: ConnectionProfile;
  connections: ConnectionProfile[];
  onSplit: (direction: MosaicDirection, connection: ConnectionProfile) => void;
  onClose: () => void;
  onConnectionsChanged: () => void;
}

export function PaneLeaf({ path, connection, connections, onSplit, onClose, onConnectionsChanged }: PaneLeafProps) {
  const [sessionId, setSessionId] = useState<string | null>(null);
  const sftp = useResizablePanel({
    defaultSize: 320,
    minSize: 200,
    maxSize: 900,
    collapseThreshold: 120,
    direction: "grow-left",
    defaultOpen: false,
  });
  // Docked below the terminal rather than beside it: the process table needs the
  // pane's full width for its columns, where a side panel would crush them.
  const monitor = useResizablePanel({
    defaultSize: 340,
    minSize: 140,
    maxSize: 1400,
    collapseThreshold: 100,
    direction: "grow-up",
    defaultOpen: false,
  });
  const [monitorMaximized, setMonitorMaximized] = useState(false);
  const focusTerminal = useRef<(() => void) | null>(null);

  // Typed into the pane's own shell, not run on a side channel: apt's raw output,
  // the sudo prompt and dpkg's config-file questions all stay in front of the user,
  // and a dropped connection mid-upgrade is visible instead of silent.
  function runHostAction(action: HostAction, asRoot: boolean) {
    if (!sessionId) return;
    setMonitorMaximized(false);
    void updatesRun(sessionId, action, asRoot);
    focusTerminal.current?.();
  }

  return (
    <MosaicWindow<string>
      path={path}
      title={connection.name}
      toolbarControls={
        <PaneToolbar
          connections={connections}
          onSplit={onSplit}
          onClose={onClose}
          sftpOpen={sftp.open}
          onToggleSftp={() => sftp.setOpen((open) => !open)}
          monitorOpen={monitor.open}
          onToggleMonitor={() => monitor.setOpen((open) => !open)}
          leading={
            <UpdatesIndicator
              sessionId={sessionId}
              manageUpdates={connection.manage_updates}
              onRun={runHostAction}
            />
          }
        />
      }
    >
      <div className="flex h-full">
        <div className="flex min-w-0 flex-1 flex-col">
          {/* Hidden, never unmounted, when the monitor is filling the pane: the
              terminal's SSH session has to survive being out of sight. */}
          <div className={cn("min-h-0 flex-1", monitorMaximized && monitor.open && "hidden")}>
            <TerminalPane
              profile={connection}
              onClosed={onClose}
              onSessionId={setSessionId}
              focusRef={focusTerminal}
            />
          </div>

          {monitor.open && sessionId && (
            <>
              {!monitorMaximized && (
                <div
                  className="group relative h-1 shrink-0 cursor-row-resize bg-border hover:bg-accent"
                  onMouseDown={monitor.handleMouseDown}
                >
                  <Button
                    size="icon-xs"
                    variant="secondary"
                    title="Hide host monitor"
                    className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 opacity-0 group-hover:opacity-100"
                    onClick={() => monitor.setOpen(false)}
                  >
                    <ChevronsDown />
                  </Button>
                </div>
              )}
              <MonitorPanel
                sessionId={sessionId}
                height={monitor.size}
                maximized={monitorMaximized}
                onToggleMaximized={() => setMonitorMaximized((maximized) => !maximized)}
                onClose={() => {
                  setMonitorMaximized(false);
                  monitor.setOpen(false);
                }}
              />
            </>
          )}
        </div>

        {sftp.open && sessionId && (
          <>
            <div
              className="group relative w-1 shrink-0 cursor-col-resize bg-border hover:bg-accent"
              onMouseDown={sftp.handleMouseDown}
            >
              <Button
                size="icon-xs"
                variant="secondary"
                title="Hide SFTP browser"
                className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 opacity-0 group-hover:opacity-100"
                onClick={() => sftp.setOpen(false)}
              >
                <ChevronsRight />
              </Button>
            </div>
            <SftpPanel
              sessionId={sessionId}
              width={sftp.size}
              connection={connection}
              onConnectionsChanged={onConnectionsChanged}
            />
          </>
        )}
      </div>
    </MosaicWindow>
  );
}
