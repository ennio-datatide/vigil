import { create } from 'zustand';
import type { VigilMessage } from '../types';

/** Transient activity shown in the chat while Vigil is processing. */
export interface VigilActivity {
  id: string;
  text: string;
  sessionId?: string;
  timestamp: number;
}

interface VigilState {
  messages: VigilMessage[];
  isProcessing: boolean;
  /** Active project path for Vigil context. */
  projectPath: string | null;
  /** Transient activities shown while Vigil is working. */
  activities: VigilActivity[];
  setMessages: (messages: VigilMessage[]) => void;
  addMessage: (message: VigilMessage) => void;
  setProcessing: (processing: boolean) => void;
  setProjectPath: (path: string | null) => void;
  addActivity: (activity: VigilActivity) => void;
  clearActivities: () => void;
}

export const useVigilStore = create<VigilState>((set) => ({
  messages: [],
  isProcessing: false,
  projectPath: null,
  activities: [],
  setMessages: (messages) => set({ messages }),
  addMessage: (message) => set((state) => ({ messages: [...state.messages, message] })),
  setProcessing: (processing) => set({ isProcessing: processing }),
  setProjectPath: (path) => set({ projectPath: path }),
  addActivity: (activity) =>
    set((state) => {
      if (state.activities.some((a) => a.id === activity.id)) return state;
      return { activities: [...state.activities, activity] };
    }),
  clearActivities: () => set({ activities: [] }),
}));
