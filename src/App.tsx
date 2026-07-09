import { useEffect, useState } from "react";

import { ConnectionEditorDialog } from "@/components/ConnectionEditorDialog";
import { ConnectionList } from "@/components/ConnectionList";
import { Workspace } from "@/components/Workspace";
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
      <ConnectionList
        connections={connections}
        activeId={null}
        onConnect={openNewTab}
        onEdit={handleEdit}
        onDuplicate={handleDuplicate}
        onDelete={handleDelete}
        onAdd={handleAdd}
      />

      <Workspace
        tabs={tabs}
        activeTabId={activeTabId}
        connections={connections}
        onActivateTab={setActiveTabId}
        onCloseTab={closeTab}
        onUpdateTab={updateTab}
      />

      <ConnectionEditorDialog
        open={editorOpen}
        onOpenChange={setEditorOpen}
        connection={editingConnection}
        onSaved={handleSaved}
      />
    </main>
  );
}

export default App;
