import { Mosaic, type MosaicBranch, type MosaicDirection, type MosaicNode } from "react-mosaic-component";
import { X } from "lucide-react";

import { Button } from "@/components/ui/button";
import { PaneLeaf } from "@/components/PaneLeaf";
import { removePane, splitPane } from "@/lib/workspace";
import type { ConnectionProfile } from "@/types/connection";
import type { Tab } from "@/types/workspace";

interface WorkspaceProps {
  tabs: Tab[];
  activeTabId: string | null;
  connections: ConnectionProfile[];
  onActivateTab: (tabId: string) => void;
  onCloseTab: (tabId: string) => void;
  onUpdateTab: (tabId: string, updater: (tab: Tab) => Tab | null) => void;
}

export function Workspace({
  tabs,
  activeTabId,
  connections,
  onActivateTab,
  onCloseTab,
  onUpdateTab,
}: WorkspaceProps) {
  return (
    <div className="flex h-full flex-1 flex-col overflow-hidden">
      {tabs.length > 0 && (
        <div className="flex items-center gap-1 border-b border-border bg-card px-2 py-1">
          {tabs.map((tab) => (
            <div
              key={tab.id}
              className={`group flex cursor-pointer items-center gap-1 rounded-md px-2 py-1 text-sm ${
                tab.id === activeTabId ? "bg-muted" : "hover:bg-muted/50"
              }`}
              onClick={() => onActivateTab(tab.id)}
            >
              <span className="max-w-40 truncate">{tab.title}</span>
              <Button
                size="icon-xs"
                variant="ghost"
                className="opacity-0 group-hover:opacity-100"
                onClick={(e) => {
                  e.stopPropagation();
                  onCloseTab(tab.id);
                }}
              >
                <X />
              </Button>
            </div>
          ))}
        </div>
      )}

      <div className="relative flex-1 overflow-hidden">
        {tabs.length === 0 && (
          <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
            Select a connection to start a session.
          </div>
        )}

        {tabs.map((tab) => (
          <div key={tab.id} className={tab.id === activeTabId ? "absolute inset-0" : "hidden"}>
            <TabMosaic
              tab={tab}
              connections={connections}
              onUpdate={(updater) => onUpdateTab(tab.id, updater)}
            />
          </div>
        ))}
      </div>
    </div>
  );
}

interface TabMosaicProps {
  tab: Tab;
  connections: ConnectionProfile[];
  onUpdate: (updater: (tab: Tab) => Tab | null) => void;
}

function TabMosaic({ tab, connections, onUpdate }: TabMosaicProps) {
  function handleLayoutChange(layout: MosaicNode<string> | null) {
    if (!layout) return;
    onUpdate((current) => ({ ...current, layout }));
  }

  function handleSplit(path: MosaicBranch[], direction: MosaicDirection, connection: ConnectionProfile) {
    onUpdate((current) => splitPane(current, path, direction, connection));
  }

  function handleClosePane(path: MosaicBranch[]) {
    onUpdate((current) => removePane(current, path));
  }

  return (
    <Mosaic<string>
      value={tab.layout}
      onChange={handleLayoutChange}
      className="sshmanager-mosaic"
      renderTile={(paneId, path) => {
        const pane = tab.panes[paneId];
        const connection = connections.find((c) => c.id === pane?.connectionId);

        if (!pane || !connection) {
          return (
            <div className="flex h-full items-center justify-center bg-card text-sm text-muted-foreground">
              Connection no longer exists
            </div>
          );
        }

        return (
          <PaneLeaf
            key={paneId}
            path={path}
            connection={connection}
            connections={connections}
            onSplit={(direction, conn) => handleSplit(path, direction, conn)}
            onClose={() => handleClosePane(path)}
          />
        );
      }}
    />
  );
}
