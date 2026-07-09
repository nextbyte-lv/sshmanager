import {
  createRemoveUpdate,
  updateTree,
  type MosaicBranch,
  type MosaicDirection,
} from "react-mosaic-component";

import type { ConnectionProfile } from "@/types/connection";
import type { Tab } from "@/types/workspace";

export function createTab(connection: ConnectionProfile): Tab {
  const paneId = crypto.randomUUID();
  return {
    id: crypto.randomUUID(),
    title: connection.name,
    layout: paneId,
    panes: { [paneId]: { id: paneId, connectionId: connection.id } },
  };
}

export function splitPane(
  tab: Tab,
  path: MosaicBranch[],
  direction: MosaicDirection,
  connection: ConnectionProfile,
): Tab {
  const existingLeaf = getNodeAtPath(tab.layout, path);
  if (existingLeaf === null || typeof existingLeaf !== "string") {
    return tab;
  }

  const newPaneId = crypto.randomUUID();
  const newLayout = updateTree(tab.layout, [
    {
      path,
      spec: {
        $set: {
          direction,
          first: existingLeaf,
          second: newPaneId,
        },
      },
    },
  ]);

  return {
    ...tab,
    layout: newLayout,
    panes: {
      ...tab.panes,
      [newPaneId]: { id: newPaneId, connectionId: connection.id },
    },
  };
}

export function removePane(tab: Tab, path: MosaicBranch[]): Tab | null {
  const removedId = getNodeAtPath(tab.layout, path);
  if (removedId === null || typeof removedId !== "string") {
    return tab;
  }

  if (path.length === 0) {
    // Removing the only pane in the tab closes the tab.
    return null;
  }

  const newLayout = updateTree(tab.layout, [createRemoveUpdate(tab.layout, path)]);
  const panes = { ...tab.panes };
  delete panes[removedId];

  return { ...tab, layout: newLayout, panes };
}

function getNodeAtPath(node: Tab["layout"], path: MosaicBranch[]): Tab["layout"] | null {
  let current: Tab["layout"] | null = node;
  for (const branch of path) {
    if (current === null || typeof current === "string") return null;
    current = current[branch];
  }
  return current;
}
