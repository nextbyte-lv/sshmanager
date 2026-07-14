import { useEffect, useMemo, useState } from "react";
import { PanelLeftOpen } from "lucide-react";

import { ConnectionEditorDialog } from "@/components/ConnectionEditorDialog";
import { ConnectionList } from "@/components/ConnectionList";
import { ConnectionsSettingsDialog } from "@/components/ConnectionsSettingsDialog";
import { Workspace } from "@/components/Workspace";
import { Button } from "@/components/ui/button";
import { useResizablePanel } from "@/hooks/useResizablePanel";
import { deleteConnection, duplicateConnection, listConnections } from "@/lib/tauri";
import { createTab } from "@/lib/workspace";
import type { ConnectionProfile } from "@/types/connection";
import type { Tab } from "@/types/workspace";

function App() {
  const [connections, setConnections] = useState<ConnectionProfile[]>([]);
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [activeTabId, setActiveTabId] = useState<string | null>(null);
  const [editorOpen, setEditorOpen] = useState(false);
  const [editingConnection, setEditingConnection] = useState<ConnectionProfile | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const sidebar = useResizablePanel({
    defaultWidth: 288,
    minWidth: 200,
    maxWidth: 600,
    collapseThreshold: 120,
    direction: "grow-right",
  });

  async function refresh() {
    setConnections(await listConnections());
  }

  useEffect(() => {
    void refresh();
  }, []);

  function handleAdd() {
    setEditingConnection(null);
    setEditorOpen(true);
  }

  function handleEdit(profile: ConnectionProfile) {
    setEditingConnection(profile);
    setEditorOpen(true);
  }

  async function handleDuplicate(profile: ConnectionProfile) {
    await duplicateConnection(profile.id);
    await refresh();
  }

  async function handleDelete(profile: ConnectionProfile) {
    if (!window.confirm(`Delete "${profile.name}"? This also removes its saved credential.`)) {
      return;
    }
    await deleteConnection(profile.id);
    await refresh();
  }

  function handleSaved() {
    void refresh();
  }

  const activeConnectionCounts = useMemo(() => {
    const counts: Record<string, number> = {};
    for (const tab of tabs) {
      for (const pane of Object.values(tab.panes)) {
        counts[pane.connectionId] = (counts[pane.connectionId] ?? 0) + 1;
      }
    }
    return counts;
  }, [tabs]);

  function openNewTab(connection: ConnectionProfile) {
    const tab = createTab(connection);
    setTabs((prev) => [...prev, tab]);
    setActiveTabId(tab.id);
  }

  function closeTab(tabId: string) {
    const next = tabs.filter((t) => t.id !== tabId);
    setTabs(next);
    if (activeTabId === tabId) {
      setActiveTabId(next.length ? next[next.length - 1].id : null);
    }
  }

  function updateTab(tabId: string, updater: (tab: Tab) => Tab | null) {
    const next = tabs.flatMap((t) => {
      if (t.id !== tabId) return [t];
      const updated = updater(t);
      return updated ? [updated] : [];
    });
    setTabs(next);
    if (next.length !== tabs.length && activeTabId === tabId) {
      setActiveTabId(next.length ? next[next.length - 1].id : null);
    }
  }

  return (
    <main className="flex h-screen w-screen overflow-hidden bg-background text-foreground">
      {sidebar.open ? (
        <>
          <ConnectionList
            connections={connections}
            activeId={null}
            activeCounts={activeConnectionCounts}
            width={sidebar.width}
            onConnect={openNewTab}
            onEdit={handleEdit}
            onDuplicate={handleDuplicate}
            onDelete={handleDelete}
            onAdd={handleAdd}
            onOpenSettings={() => setSettingsOpen(true)}
          />
          <div
            className="w-1 shrink-0 cursor-col-resize bg-border hover:bg-accent"
            onMouseDown={sidebar.handleMouseDown}
          />
        </>
      ) : (
        <div className="flex w-6 shrink-0 flex-col items-center border-r border-border bg-card pt-2">
          <Button
            size="icon-xs"
            variant="ghost"
            title="Show connections"
            onClick={() => sidebar.setOpen(true)}
          >
            <PanelLeftOpen />
          </Button>
        </div>
      )}

      <Workspace
        tabs={tabs}
        activeTabId={activeTabId}
        connections={connections}
        onActivateTab={setActiveTabId}
        onCloseTab={closeTab}
        onUpdateTab={updateTab}
        onConnectionsChanged={refresh}
      />

      <ConnectionEditorDialog
        open={editorOpen}
        onOpenChange={setEditorOpen}
        connection={editingConnection}
        onSaved={handleSaved}
      />

      <ConnectionsSettingsDialog
        open={settingsOpen}
        onOpenChange={setSettingsOpen}
        connections={connections}
        onImported={refresh}
      />
    </main>
  );
}

export default App;
