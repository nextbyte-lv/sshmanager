import { useState } from "react";

import { DropdownMenuContent, DropdownMenuItem } from "@/components/ui/dropdown-menu";
import type { ConnectionProfile } from "@/types/connection";

interface ConnectionPickerMenuProps {
  connections: ConnectionProfile[];
  onSelect: (connection: ConnectionProfile) => void;
}

export function ConnectionPickerMenu({ connections, onSelect }: ConnectionPickerMenuProps) {
  const [query, setQuery] = useState("");

  const filtered = query.trim()
    ? connections.filter((c) => `${c.name} ${c.host}`.toLowerCase().includes(query.trim().toLowerCase()))
    : connections;

  return (
    <DropdownMenuContent className="w-56">
      <input
        autoFocus
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        onKeyDown={(e) => e.stopPropagation()}
        placeholder="Filter…"
        className="mb-1 w-full rounded-md border border-input bg-transparent px-1.5 py-1 text-sm outline-none focus-visible:border-ring"
      />
      {filtered.length === 0 && (
        <p className="px-1.5 py-1 text-sm text-muted-foreground">No connections</p>
      )}
      {filtered.map((connection) => (
        <DropdownMenuItem key={connection.id} onClick={() => onSelect(connection)}>
          <div className="flex min-w-0 flex-col">
            <span className="truncate">{connection.name}</span>
            <span className="truncate text-xs text-muted-foreground">
              {connection.username}@{connection.host}
            </span>
          </div>
        </DropdownMenuItem>
      ))}
    </DropdownMenuContent>
  );
}
