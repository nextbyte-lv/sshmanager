import { useState } from "react";
import { MosaicWindow, type MosaicBranch, type MosaicDirection } from "react-mosaic-component";
import { ChevronsRight } from "lucide-react";

import { PaneToolbar } from "@/components/PaneToolbar";
import { SftpPanel } from "@/components/SftpPanel";
import { TerminalPane } from "@/components/TerminalPane";
import { Button } from "@/components/ui/button";
import { useResizablePanel } from "@/hooks/useResizablePanel";
import type { ConnectionProfile } from "@/types/connection";

interface PaneLeafProps {
  path: MosaicBranch[];
  connection: ConnectionProfile;
  connections: ConnectionProfile[];
  onSplit: (direction: MosaicDirection, connection: ConnectionProfile) => void;
  onClose: () => void;
  onConnectionsChanged: () => void;
}

export function PaneLeaf({ path, connection, connections, onSplit, onClose, onConnectionsChanged }: PaneLeafProps) {
  const [sessionId, setSessionId] = useState<string | null>(null);
  const sftp = useResizablePanel({
    defaultWidth: 320,
    minWidth: 200,
    maxWidth: 900,
    collapseThreshold: 120,
    direction: "grow-left",
    defaultOpen: false,
  });

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
        />
      }
    >
      <div className="flex h-full">
        <div className="min-w-0 flex-1">
          <TerminalPane profile={connection} onClosed={onClose} onSessionId={setSessionId} />
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
              width={sftp.width}
              connection={connection}
              onConnectionsChanged={onConnectionsChanged}
            />
          </>
        )}
      </div>
    </MosaicWindow>
  );
}
