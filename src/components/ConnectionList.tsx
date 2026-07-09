import { useMemo, useState } from "react";
import { Copy, Pencil, Plus, Search, Settings, Trash2 } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import type { ConnectionProfile } from "@/types/connection";

interface ConnectionListProps {
  connections: ConnectionProfile[];
  activeId: string | null;
  onConnect: (profile: ConnectionProfile) => void;
  onEdit: (profile: ConnectionProfile) => void;
  onDuplicate: (profile: ConnectionProfile) => void;
  onDelete: (profile: ConnectionProfile) => void;
  onAdd: () => void;
  onOpenSettings: () => void;
}

export function ConnectionList({
  connections,
  activeId,
  onConnect,
  onEdit,
  onDuplicate,
  onDelete,
  onAdd,
  onOpenSettings,
}: ConnectionListProps) {
  const [query, setQuery] = useState("");

  const groups = useMemo(() => {
    const q = query.trim().toLowerCase();
    const filtered = q
      ? connections.filter((c) =>
          [c.name, c.host, c.username, ...c.tags].some((field) => field.toLowerCase().includes(q)),
        )
      : connections;

    const byTag = new Map<string, ConnectionProfile[]>();
    for (const connection of filtered) {
      const tags = connection.tags.length ? connection.tags : ["Ungrouped"];
      for (const tag of tags) {
        if (!byTag.has(tag)) byTag.set(tag, []);
        byTag.get(tag)!.push(connection);
      }
    }

    return Array.from(byTag.entries()).sort(([a], [b]) => {
      if (a === "Ungrouped") return 1;
      if (b === "Ungrouped") return -1;
      return a.localeCompare(b);
    });
  }, [connections, query]);

  return (
    <div className="flex h-full w-72 flex-col border-r border-border bg-card">
      <div className="flex items-center gap-2 border-b border-border p-3">
        <div className="relative flex-1">
          <Search className="pointer-events-none absolute top-1/2 left-2 size-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search connections…"
            className="pl-7"
          />
        </div>
        <Button size="icon-sm" variant="outline" onClick={onAdd} title="Add connection">
          <Plus />
        </Button>
        <Button size="icon-sm" variant="outline" onClick={onOpenSettings} title="Import / export connections">
          <Settings />
        </Button>
      </div>

      <ScrollArea className="flex-1">
        <div className="flex flex-col gap-3 p-2">
          {groups.length === 0 && (
            <p className="p-3 text-sm text-muted-foreground">No connections yet.</p>
          )}
          {groups.map(([tag, items]) => (
            <div key={tag}>
              <p className="px-2 pb-1 text-xs font-medium tracking-wide text-muted-foreground uppercase">
                {tag}
              </p>
              <div className="flex flex-col gap-0.5">
                {items.map((connection) => (
                  <div
                    key={`${tag}-${connection.id}`}
                    className={`group flex items-center gap-1 rounded-lg px-2 py-1.5 hover:bg-muted ${
                      activeId === connection.id ? "bg-muted" : ""
                    }`}
                  >
                    <button
                      type="button"
                      className="min-w-0 flex-1 text-left"
                      onClick={() => onConnect(connection)}
                    >
                      <div className="truncate text-sm font-medium">{connection.name}</div>
                      <div className="truncate text-xs text-muted-foreground">
                        {connection.username}@{connection.host}:{connection.port}
                      </div>
                    </button>
                    <div className="flex shrink-0 opacity-0 group-hover:opacity-100">
                      <Button
                        size="icon-xs"
                        variant="ghost"
                        title="Edit"
                        onClick={() => onEdit(connection)}
                      >
                        <Pencil />
                      </Button>
                      <Button
                        size="icon-xs"
                        variant="ghost"
                        title="Duplicate"
                        onClick={() => onDuplicate(connection)}
                      >
                        <Copy />
                      </Button>
                      <Button
                        size="icon-xs"
                        variant="ghost"
                        title="Delete"
                        onClick={() => onDelete(connection)}
                      >
                        <Trash2 />
                      </Button>
                    </div>
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>
      </ScrollArea>
    </div>
  );
}
