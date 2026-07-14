import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { Loader2, RefreshCw } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { Button } from "@/components/ui/button";
import { closeSession, openSession, resizeSession, sendInput } from "@/lib/tauri";
import type { ConnectionProfile, TerminalEvent } from "@/types/connection";

const DEFAULT_TERMINAL_BACKGROUND = "#0b0f19";

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
  const [status, setStatus] = useState<"connecting" | "connected" | "error">("connecting");
  const reconnectRef = useRef(() => {});

  useEffect(() => {
    setStatus("connecting");
    const container = containerRef.current;
    if (!container) return;

    const term = new Terminal({
      cursorBlink: true,
      fontFamily: '"Cascadia Mono", Consolas, monospace',
      fontSize: 13,
      theme: { background: profile.color || DEFAULT_TERMINAL_BACKGROUND },
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

    const selectionDisposable = term.onSelectionChange(() => {
      const selection = term.getSelection();
      if (selection) void navigator.clipboard.writeText(selection).catch(() => {});
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

    const connect = () => {
      setStatus("connecting");
      openSession(profile.id, term.cols, term.rows, (event: TerminalEvent) => {
        if (event.type === "data") {
          term.write(event.data);
        } else if (event.type === "closed") {
          term.write("\r\n\x1b[90m[session closed]\x1b[0m\r\n");
          sessionId = null;
          onClosedRef.current?.();
        } else if (event.type === "error") {
          term.write(`\r\n\x1b[31m${event.message}\x1b[0m\r\n`);
          sessionId = null;
          if (!disposed) setStatus("error");
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
          setStatus("connected");
          onSessionIdRef.current?.(id);
        })
        .catch((err) => {
          term.write(`\r\n\x1b[31mFailed to connect: ${String(err)}\x1b[0m\r\n`);
          if (!disposed) setStatus("error");
        });
    };

    reconnectRef.current = () => {
      if (!sessionId) connect();
    };

    connect();

    return () => {
      disposed = true;
      resizeObserver.disconnect();
      selectionDisposable.dispose();
      dataDisposable.dispose();
      if (sessionId) void closeSession(sessionId);
      term.dispose();
    };
  }, [profile.id]);

  return (
    <div className="relative h-full w-full">
      <div ref={containerRef} className="h-full w-full" />
      {status === "connecting" && (
        <div
          className="absolute inset-0 flex flex-col items-center justify-center gap-2 text-sm text-muted-foreground"
          style={{ backgroundColor: profile.color || DEFAULT_TERMINAL_BACKGROUND }}
        >
          <Loader2 className="h-5 w-5 animate-spin" />
          <span>
            Connecting to {profile.host}:{profile.port}...
          </span>
        </div>
      )}
      {status === "error" && (
        <div className="absolute inset-x-0 bottom-0 flex items-center justify-center gap-2 bg-black/60 p-2 backdrop-blur-sm">
          <span className="text-sm text-muted-foreground">Connection lost</span>
          <Button size="sm" variant="outline" onClick={() => reconnectRef.current()}>
            <RefreshCw data-icon="inline-start" />
            Reconnect
          </Button>
        </div>
      )}
    </div>
  );
}
