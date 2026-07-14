import { useCallback, useRef, useState } from "react";

interface UseResizablePanelOptions {
  defaultWidth: number;
  minWidth: number;
  maxWidth: number;
  /** Dragging narrower than this snaps the panel closed instead of leaving it stuck too thin to use. */
  collapseThreshold: number;
  /** Which way the panel grows relative to the divider's drag direction. */
  direction: "grow-left" | "grow-right";
  defaultOpen?: boolean;
}

export function useResizablePanel({
  defaultWidth,
  minWidth,
  maxWidth,
  collapseThreshold,
  direction,
  defaultOpen = true,
}: UseResizablePanelOptions) {
  const [width, setWidth] = useState(defaultWidth);
  const [open, setOpen] = useState(defaultOpen);
  const dragState = useRef<{ startX: number; startWidth: number } | null>(null);

  const handleMouseMove = useCallback(
    (e: MouseEvent) => {
      if (!dragState.current) return;
      const rawDelta = e.clientX - dragState.current.startX;
      const delta = direction === "grow-right" ? rawDelta : -rawDelta;
      const raw = dragState.current.startWidth + delta;
      if (raw < collapseThreshold) {
        setOpen(false);
        return;
      }
      setWidth(Math.min(maxWidth, Math.max(minWidth, raw)));
    },
    [direction, collapseThreshold, minWidth, maxWidth],
  );

  const handleMouseUp = useCallback(() => {
    dragState.current = null;
    document.removeEventListener("mousemove", handleMouseMove);
    document.removeEventListener("mouseup", handleMouseUp);
  }, [handleMouseMove]);

  const handleMouseDown = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault();
      dragState.current = { startX: e.clientX, startWidth: width };
      document.addEventListener("mousemove", handleMouseMove);
      document.addEventListener("mouseup", handleMouseUp);
    },
    [width, handleMouseMove, handleMouseUp],
  );

  return { width, open, setOpen, handleMouseDown };
}
