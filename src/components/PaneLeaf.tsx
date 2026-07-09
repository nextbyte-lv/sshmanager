import { useState } from "react";
import { MosaicWindow, type MosaicBranch, type MosaicDirection } from "react-mosaic-component";

import { PaneToolbar } from "@/components/PaneToolbar";
import { SftpPanel } from "@/components/SftpPanel";
import { TerminalPane } from "@/components/TerminalPane";
import type { ConnectionProfile } from "@/types/connection";

interface PaneLeafProps {
  path: MosaicBranch[];
  connection: ConnectionProfile;
  connections: ConnectionProfile[];
  onSplit: (direction: MosaicDirection, connection: ConnectionProfile) => void;
  onClose: () => void;
}

export function PaneLeaf({ path, connection, connections, onSplit, onClose }: PaneLeafProps) {
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [sftpOpen, setSftpOpen] = useState(false);

  return (
    <MosaicWindow<string>
      path={path}
      title={connection.name}
      toolbarControls={
        <PaneToolbar
          connections={connections}
          onSplit={onSplit}
          onClose={onClose}
          sftpOpen={sftpOpen}
          onToggleSftp={() => setSftpOpen((open) => !open)}
        />
      }
    >
      <div className="flex h-full">
        <div className="min-w-0 flex-1">
          <TerminalPane profile={connection} onClosed={onClose} onSessionId={setSessionId} />
        </div>
        {sftpOpen && sessionId && <SftpPanel sessionId={sessionId} />}
      </div>
    </MosaicWindow>
  );
}
