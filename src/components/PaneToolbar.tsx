import type { MosaicDirection } from "react-mosaic-component";
import { Columns2, FolderOpen, Rows2, X } from "lucide-react";

import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { ConnectionPickerMenu } from "@/components/ConnectionPickerMenu";
import type { ConnectionProfile } from "@/types/connection";

interface PaneToolbarProps {
  connections: ConnectionProfile[];
  onSplit: (direction: MosaicDirection, connection: ConnectionProfile) => void;
  onClose: () => void;
  sftpOpen: boolean;
  onToggleSftp: () => void;
}

export function PaneToolbar({ connections, onSplit, onClose, sftpOpen, onToggleSftp }: PaneToolbarProps) {
  return (
    <div className="flex items-center gap-0.5">
      <Button
        size="icon-xs"
        variant={sftpOpen ? "secondary" : "ghost"}
        title="Toggle SFTP browser"
        onClick={onToggleSftp}
      >
        <FolderOpen />
      </Button>

      <DropdownMenu>
        <DropdownMenuTrigger render={<Button size="icon-xs" variant="ghost" title="Split right" />}>
          <Columns2 />
        </DropdownMenuTrigger>
        <ConnectionPickerMenu connections={connections} onSelect={(c) => onSplit("row", c)} />
      </DropdownMenu>

      <DropdownMenu>
        <DropdownMenuTrigger render={<Button size="icon-xs" variant="ghost" title="Split down" />}>
          <Rows2 />
        </DropdownMenuTrigger>
        <ConnectionPickerMenu connections={connections} onSelect={(c) => onSplit("column", c)} />
      </DropdownMenu>

      <Button size="icon-xs" variant="ghost" title="Close pane" onClick={onClose}>
        <X />
      </Button>
    </div>
  );
}
