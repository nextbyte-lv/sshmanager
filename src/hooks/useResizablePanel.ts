import { useCallback, useRef, useState } from "react";

/**
 * Which way the panel grows relative to the divider's drag direction. The
 * `grow-left`/`grow-right` pair track the pointer's X, the `grow-up`/`grow-down`
 * pair its Y — so the same hook drives the SFTP browser docked to the right of a
 * pane and the host monitor docked below it.
 */
type GrowDirection = "grow-left" | "grow-right" | "grow-up" | "grow-down";

interface UseResizablePanelOptions {
  defaultSize: number;
  minSize: number;
  maxSize: number;
  /** Dragging smaller than this snaps the panel closed instead of leaving it stuck too small to use. */
  collapseThreshold: number;
  direction: GrowDirection;
  defaultOpen?: boolean;
}

export function useResizablePanel({
  defaultSize,
  minSize,
  maxSize,
  collapseThreshold,
  direction,
  defaultOpen = true,
}: UseResizablePanelOptions) {
  const [size, setSize] = useState(defaultSize);
  const [open, setOpen] = useState(defaultOpen);
  const dragState = useRef<{ start: number; startSize: number } | null>(null);

  const vertical = direction === "grow-up" || direction === "grow-down";

  const handleMouseMove = useCallback(
    (e: MouseEvent) => {
      if (!dragState.current) return;
      const rawDelta = (vertical ? e.clientY : e.clientX) - dragState.current.start;
      // The two "grow towards the pointer's origin" directions read the drag
      // inverted: the divider sits on the panel's leading edge.
      const delta = direction === "grow-right" || direction === "grow-down" ? rawDelta : -rawDelta;
      const raw = dragState.current.startSize + delta;
      if (raw < collapseThreshold) {
        setOpen(false);
        return;
      }
      setSize(Math.min(maxSize, Math.max(minSize, raw)));
    },
    [direction, vertical, collapseThreshold, minSize, maxSize],
  );

  const handleMouseUp = useCallback(() => {
    dragState.current = null;
    document.removeEventListener("mousemove", handleMouseMove);
    document.removeEventListener("mouseup", handleMouseUp);
  }, [handleMouseMove]);

  const handleMouseDown = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault();
      dragState.current = { start: vertical ? e.clientY : e.clientX, startSize: size };
      document.addEventListener("mousemove", handleMouseMove);
      document.addEventListener("mouseup", handleMouseUp);
    },
    [size, vertical, handleMouseMove, handleMouseUp],
  );

  return { size, open, setOpen, handleMouseDown };
}
