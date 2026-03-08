import { create } from 'zustand';
import type { VigilMessage } from '../types';

interface VigilState {
  messages: VigilMessage[];
  isProcessing: boolean;
  setMessages: (messages: VigilMessage[]) => void;
  addMessage: (message: VigilMessage) => void;
  setProcessing: (processing: boolean) => void;
}

export const useVigilStore = create<VigilState>((set) => ({
  messages: [],
  isProcessing: false,
  setMessages: (messages) => set({ messages }),
  addMessage: (message) =>
    set((state) => ({ messages: [...state.messages, message] })),
  setProcessing: (processing) => set({ isProcessing: processing }),
}));
