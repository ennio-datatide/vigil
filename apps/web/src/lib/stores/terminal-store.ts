import { create } from 'zustand';

type PanelMode = 'closed' | 'panel' | 'fullscreen';

interface TerminalState {
  activeSessionId: string | null;
  panelMode: PanelMode;
  openSession: (sessionId: string) => void;
  closePanel: () => void;
  setPanelMode: (mode: PanelMode) => void;
  toggleFullscreen: () => void;
}

export const useTerminalStore = create<TerminalState>((set) => ({
  activeSessionId: null,
  panelMode: 'closed',
  openSession: (sessionId) => set({ activeSessionId: sessionId, panelMode: 'panel' }),
  closePanel: () => set({ activeSessionId: null, panelMode: 'closed' }),
  setPanelMode: (mode) => set({ panelMode: mode }),
  toggleFullscreen: () =>
    set((s) => ({
      panelMode: s.panelMode === 'fullscreen' ? 'panel' : 'fullscreen',
    })),
}));
