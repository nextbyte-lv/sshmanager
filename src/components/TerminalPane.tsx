import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import "@xterm/xterm/css/xterm.css";

import { closeSession, openSession, resizeSession, sendInput } from "@/lib/tauri";
import type { ConnectionProfile, TerminalEvent } from "@/types/connection";

interface TerminalPaneProps {
  profile: ConnectionProfile;
  onClosed?: (message?: string) => void;
  onSessionId?: (sessionId: string) => void;
}

export function TerminalPane({ profile, onClosed, onSessionId }: TerminalPaneProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const onClosedRef = useRef(onClosed);
  onClosedRef.current = onClosed;
  const onSessionIdRef = useRef(onSessionId);
  onSessionIdRef.current = onSessionId;

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const term = new Terminal({
      cursorBlink: true,
      fontFamily: '"Cascadia Mono", Consolas, monospace',
      fontSize: 13,
      theme: { background: "#0b0f19" },
    });
    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.loadAddon(new SearchAddon());
    term.open(container);
    fitAddon.fit();

    let sessionId: string | null = null;
    let disposed = false;

    const dataDisposable = term.onData((data) => {
      if (sessionId) void sendInput(sessionId, data);
    });

    const resizeObserver = new ResizeObserver(() => {
      // Hidden panes (e.g. an inactive tab) report a zero-size box; fitting
      // against that would collapse the terminal to 0 cols/rows. Skip until
      // it's actually visible again, at which point this fires once more
      // with the real size and resyncs the PTY.
      if (container.clientWidth === 0 || container.clientHeight === 0) return;
      fitAddon.fit();
      if (sessionId) void resizeSession(sessionId, term.cols, term.rows);
    });
    resizeObserver.observe(container);

    openSession(profile.id, term.cols, term.rows, (event: TerminalEvent) => {
      if (event.type === "data") {
        term.write(event.data);
      } else if (event.type === "closed") {
        term.write("\r\n\x1b[90m[session closed]\x1b[0m\r\n");
        onClosedRef.current?.();
      } else if (event.type === "error") {
        term.write(`\r\n\x1b[31m${event.message}\x1b[0m\r\n`);
        onClosedRef.current?.(event.message);
      } else if (event.type === "reconnecting") {
        term.write(
          `\r\n\x1b[33m[connection lost, reconnecting (attempt ${event.attempt}/${event.max_attempts})...]\x1b[0m\r\n`,
        );
      } else if (event.type === "reconnected") {
        term.write("\r\n\x1b[32m[reconnected]\x1b[0m\r\n");
      }
    })
      .then((id) => {
        if (disposed) {
          void closeSession(id);
          return;
        }
        sessionId = id;
        onSessionIdRef.current?.(id);
      })
      .catch((err) => {
        term.write(`\r\n\x1b[31mFailed to connect: ${String(err)}\x1b[0m\r\n`);
        onClosedRef.current?.(String(err));
      });

    return () => {
      disposed = true;
      resizeObserver.disconnect();
      dataDisposable.dispose();
      if (sessionId) void closeSession(sessionId);
      term.dispose();
    };
  }, [profile.id]);

  return (
    <div className="h-full w-full p-2">
      <div ref={containerRef} className="h-full w-full" />
    </div>
  );
}
