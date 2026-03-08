'use client';
import { useCallback, useEffect, useRef } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { useSessionsQuery } from '../api';
import { useSessionStore } from '../stores/session-store';
import type { WsMessageExtended } from '../types';
import { wsUrl } from '../ws-url';

export function useDashboardWs() {
  const wsRef = useRef<WebSocket | null>(null);
  const reconnectTimeoutRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const reconnectAttemptRef = useRef(0);
  const unmountedRef = useRef(false);
  const { setSession, removeSession, syncAll } = useSessionStore();
  const queryClient = useQueryClient();

  // REST: always fetch fresh sessions on mount so dashboard is never stale
  const { data: restSessions } = useSessionsQuery();

  useEffect(() => {
    if (restSessions) {
      syncAll(restSessions);
    }
  }, [restSessions, syncAll]);

  const connect = useCallback(() => {
    if (unmountedRef.current) return;

    const ws = new WebSocket(wsUrl('/ws/dashboard'));
    wsRef.current = ws;

    ws.onmessage = (event) => {
      const msg: WsMessageExtended = JSON.parse(event.data);
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
        case 'child_spawned':
        case 'child_completed':
          // Refresh sessions to pick up parent/child state changes
          queryClient.invalidateQueries({ queryKey: ['sessions'] });
          break;
        case 'status_changed':
          // Session status changed — sessions will be updated via session_update
          // This event is useful for triggering blocker UI in Vigil chat
          break;
        case 'memory_updated':
          queryClient.invalidateQueries({ queryKey: ['memory'] });
          break;
        case 'acta_refreshed':
          queryClient.invalidateQueries({ queryKey: ['vigil-status'] });
          break;
      }
    };

    ws.onopen = () => {
      reconnectAttemptRef.current = 0;
    };

    ws.onclose = () => {
      if (unmountedRef.current) return;
      const delay = Math.min(1000 * 2 ** reconnectAttemptRef.current, 30000);
      reconnectAttemptRef.current++;
      reconnectTimeoutRef.current = setTimeout(connect, delay);
    };

    ws.onerror = () => {
      ws.close();
    };
  }, [setSession, removeSession, syncAll, queryClient]);

  useEffect(() => {
    unmountedRef.current = false;
    connect();
    return () => {
      unmountedRef.current = true;
      if (reconnectTimeoutRef.current) clearTimeout(reconnectTimeoutRef.current);
      wsRef.current?.close();
    };
  }, [connect]);
}
