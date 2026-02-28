import { create } from 'zustand';
import type { Session } from '../types';

interface SessionState {
  sessions: Record<string, Session>;
  initialized: boolean;
  setSession: (session: Session) => void;
  removeSession: (id: string) => void;
  syncAll: (sessions: Session[]) => void;
}

export const useSessionStore = create<SessionState>((set) => ({
  sessions: {},
  initialized: false,
  setSession: (session) =>
    set((state) => ({
      sessions: { ...state.sessions, [session.id]: session },
    })),
  removeSession: (id) =>
    set((state) => {
      const { [id]: _, ...rest } = state.sessions;
      return { sessions: rest };
    }),
  syncAll: (sessions) =>
    set({
      initialized: true,
      sessions: Object.fromEntries(sessions.map((s) => [s.id, s])),
    }),
}));
