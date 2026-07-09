import type { MosaicNode } from "react-mosaic-component";

export interface PaneState {
  id: string;
  connectionId: string;
}

export interface Tab {
  id: string;
  title: string;
  layout: MosaicNode<string>;
  panes: Record<string, PaneState>;
}
