'use client';
import { useCallback, useEffect, useRef } from 'react';
import { useSessionStore } from '../stores/session-store';
import type { WsMessage } from '../types';
import { wsUrl } from '../ws-url';

export function useDashboardWs() {
  const wsRef = useRef<WebSocket | null>(null);
  const reconnectTimeoutRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const reconnectAttemptRef = useRef(0);
  const { setSession, removeSession, syncAll } = useSessionStore();

  const connect = useCallback(() => {
    const ws = new WebSocket(wsUrl('/ws/dashboard'));
    wsRef.current = ws;

    ws.onmessage = (event) => {
      const msg: WsMessage = JSON.parse(event.data);
      switch (msg.type) {
        case 'state_sync':
          syncAll(msg.sessions);
          break;
        case 'session_update':
          setSession(msg.session);
          break;
        case 'session_removed':
          removeSession(msg.sessionId);
          break;
      }
    };

    ws.onopen = () => {
      reconnectAttemptRef.current = 0;
    };

    ws.onclose = () => {
      // Exponential backoff reconnect
      const delay = Math.min(1000 * 2 ** reconnectAttemptRef.current, 30000);
      reconnectAttemptRef.current++;
      reconnectTimeoutRef.current = setTimeout(connect, delay);
    };

    ws.onerror = () => {
      ws.close();
    };
  }, [setSession, removeSession, syncAll]);

  useEffect(() => {
    connect();
    return () => {
      if (reconnectTimeoutRef.current) clearTimeout(reconnectTimeoutRef.current);
      wsRef.current?.close();
    };
  }, [connect]);
}
