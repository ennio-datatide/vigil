'use client';
import { useQueryClient } from '@tanstack/react-query';
import { useCallback, useEffect, useRef } from 'react';
import { useSessionsQuery } from '../api';
import { useSessionStore } from '../stores/session-store';
import { useVigilStore } from '../stores/vigil-store';
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
      const { isProcessing, addActivity } = useVigilStore.getState();
      switch (msg.type) {
        case 'state_sync':
          syncAll(msg.sessions);
          break;
        case 'session_update': {
          const prevSession = useSessionStore.getState().sessions[msg.session.id];
          setSession(msg.session);
          if (isProcessing) {
            const shortPrompt =
              msg.session.prompt.length > 60
                ? `${msg.session.prompt.slice(0, 57)}...`
                : msg.session.prompt;

            // New session spawned (not seen before, or just transitioned to running)
            if (
              msg.session.status === 'running' &&
              (!prevSession || prevSession.status === 'queued')
            ) {
              addActivity({
                id: `spawned-${msg.session.id}`,
                text: `Worker started: ${shortPrompt}`,
                sessionId: msg.session.id,
                timestamp: Date.now(),
              });
            }
            // Session finished
            if (msg.session.status === 'completed' || msg.session.status === 'failed') {
              const verb = msg.session.status === 'completed' ? 'completed' : 'failed';
              addActivity({
                id: `done-${msg.session.id}`,
                text: `Worker ${verb}: ${shortPrompt}`,
                sessionId: msg.session.id,
                timestamp: Date.now(),
              });
            }
            // Session blocked — needs user input
            if (msg.session.status === 'needs_input' || msg.session.status === 'auth_required') {
              addActivity({
                id: `blocker-${msg.session.id}`,
                text: 'Worker needs your input — click to open terminal',
                sessionId: msg.session.id,
                timestamp: Date.now(),
              });
            }
          }
          break;
        }
        case 'session_removed':
          removeSession(msg.sessionId);
          break;
        case 'child_spawned':
        case 'child_completed':
          // Refresh sessions to pick up parent/child state changes
          queryClient.invalidateQueries({ queryKey: ['sessions'] });
          break;
        case 'status_changed':
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
