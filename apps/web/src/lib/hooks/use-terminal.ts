'use client';
import { useEffect, useRef, useState } from 'react';
import { wsUrl } from '../ws-url';

export function useTerminal(
  sessionId: string,
  containerRef: React.RefObject<HTMLDivElement | null>,
) {
  const initialized = useRef(false);
  const [connected, setConnected] = useState(false);
  const [ptyAlive, setPtyAlive] = useState(true);
  const wsRef = useRef<WebSocket | null>(null);
  const ptyAliveRef = useRef(true);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    // Guard against React Strict Mode double-mount
    if (initialized.current) return;
    initialized.current = true;

    let disposed = false;
    let term: import('@xterm/xterm').Terminal | null = null;
    let ws: WebSocket | null = null;
    let fitAddon: import('@xterm/addon-fit').FitAddon | null = null;
    let resizeObserver: ResizeObserver | null = null;

    (async () => {
      try {
        // Dynamically import xterm (no SSR)
        const { Terminal } = await import('@xterm/xterm');
        const { FitAddon } = await import('@xterm/addon-fit');

        if (disposed) return;

        term = new Terminal({
          cursorBlink: true,
          disableStdin: false,
          scrollback: 5000,
          fontSize: 13,
          fontFamily: "'JetBrains Mono', 'Fira Code', 'Cascadia Code', monospace",
          theme: {
            background: '#0d1117',
            foreground: '#e6edf3',
            cursor: '#58a6ff',
            selectionBackground: '#264f78',
          },
        });

        fitAddon = new FitAddon();
        term.loadAddon(fitAddon);

        // Ensure container has dimensions before opening
        if (container.clientHeight === 0) {
          container.style.minHeight = '200px';
        }

        term.open(container);

        // Try WebGL addon for performance, fall back gracefully
        try {
          const { WebglAddon } = await import('@xterm/addon-webgl');
          if (!disposed && term) {
            const webglAddon = new WebglAddon();
            term.loadAddon(webglAddon);
          }
        } catch {
          // WebGL not supported, canvas renderer is fine
        }

        if (disposed) {
          term.dispose();
          return;
        }

        fitAddon.fit();

        // Focus terminal so it captures keyboard input immediately
        term.focus();

        // Also focus on click anywhere in the container
        container.addEventListener('mousedown', () => {
          // Use requestAnimationFrame to focus after the browser processes the click
          requestAnimationFrame(() => term?.focus());
        });

        term.write('Connecting...\r\n');

        // Connect WebSocket directly to backend (Next.js rewrites don't proxy WS)
        const url = wsUrl(`/ws/terminal/${sessionId}`);
        console.log('[useTerminal] Connecting to:', url);
        ws = new WebSocket(url);
        wsRef.current = ws;

        ws.onopen = () => {
          console.log('[useTerminal] WebSocket connected');
          setConnected(true);
          term?.write('\x1b[32mConnected.\x1b[0m\r\n\r\n');

          // Send initial resize so PTY gets correct dimensions
          if (term && ws) {
            ws.send(JSON.stringify({ type: 'resize', cols: term.cols, rows: term.rows }));
          }

          // Re-focus after connection established
          term?.focus();
        };

        ws.onmessage = (event: MessageEvent) => {
          if (typeof event.data === 'string') {
            // Check for JSON control messages from server
            try {
              const msg = JSON.parse(event.data);
              if (msg.type === 'pty_status') {
                const wasAlive = ptyAliveRef.current;
                ptyAliveRef.current = msg.alive;
                setPtyAlive(msg.alive);
                if (!msg.alive) {
                  term?.write('\r\n\x1b[33m[Session ended — terminal is read-only]\x1b[0m\r\n');
                } else if (!wasAlive) {
                  // PTY came back alive after a restart
                  term?.write('\r\n\x1b[32m[Session restarted — terminal is active]\x1b[0m\r\n');
                  term?.focus();
                }
                return;
              }
            } catch {
              // Not JSON — raw PTY output, write to terminal
            }
            term?.write(event.data);
          } else if (event.data instanceof Blob) {
            event.data.text().then((text: string) => term?.write(text));
          }
        };

        ws.onerror = (e) => {
          console.error('[useTerminal] WebSocket error:', e);
          term?.write('\r\n\x1b[31m[Connection error]\x1b[0m\r\n');
        };

        ws.onclose = (e) => {
          console.log('[useTerminal] WebSocket closed:', e.code, e.reason);
          setConnected(false);
          term?.write('\r\n\x1b[33m[Disconnected]\x1b[0m\r\n');
        };

        // Forward all keyboard input to the server via WebSocket
        term.onData((data: string) => {
          if (ws && ws.readyState === WebSocket.OPEN) {
            ws.send(JSON.stringify({ type: 'input', data }));
          } else {
            console.warn('[useTerminal] Cannot send input, WS state:', ws?.readyState);
          }
        });

        // Forward binary input (e.g. bracketed paste, special keys)
        term.onBinary((data: string) => {
          if (ws && ws.readyState === WebSocket.OPEN) {
            ws.send(JSON.stringify({ type: 'input', data }));
          }
        });

        // Resize handling — fit terminal and notify server
        const handleResize = () => {
          try {
            fitAddon?.fit();
            if (ws && ws.readyState === WebSocket.OPEN && term) {
              ws.send(JSON.stringify({ type: 'resize', cols: term.cols, rows: term.rows }));
            }
          } catch {
            /* ignore fit errors */
          }
        };

        resizeObserver = new ResizeObserver(handleResize);
        resizeObserver.observe(container);
      } catch (err) {
        console.error('[useTerminal] Failed to initialize:', err);
      }
    })();

    return () => {
      disposed = true;
      initialized.current = false;
      resizeObserver?.disconnect();
      wsRef.current = null;
      ws?.close();
      term?.dispose();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId, containerRef.current]);

  /** Send text input to the PTY (for mobile input bar). */
  const sendInput = (text: string) => {
    const ws = wsRef.current;
    if (ws && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify({ type: 'input', data: text }));
    }
  };

  return { connected, ptyAlive, sendInput };
}
