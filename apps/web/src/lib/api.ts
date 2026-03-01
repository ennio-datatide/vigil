import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { getToken } from './auth-token';
import type {
  CreatePipelineInputType,
  CreateSessionInputType,
  NotificationMessage,
  Pipeline,
  Session,
  UpdatePipelineInputType,
} from './types';

const API_BASE = ''; // Uses Next.js rewrites to proxy to Fastify

async function fetchJson<T>(url: string, init?: RequestInit): Promise<T> {
  const token = getToken();
  const headers = new Headers(init?.headers);
  if (token) {
    headers.set('Authorization', `Bearer ${token}`);
  }
  const res = await fetch(`${API_BASE}${url}`, { ...init, headers });
  if (res.status === 401) {
    if (typeof window !== 'undefined') {
      window.location.href = '/dashboard/auth';
    }
    throw new Error('Unauthorized');
  }
  if (!res.ok) throw new Error(`API error: ${res.status}`);
  return res.json();
}

export function useSessionsQuery() {
  return useQuery({
    queryKey: ['sessions'],
    queryFn: () => fetchJson<Session[]>('/api/sessions'),
  });
}

export function useCreateSession() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: CreateSessionInputType) =>
      fetchJson<Session>('/api/sessions', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(input),
      }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['sessions'] }),
  });
}

export function useCancelSession() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => fetchJson(`/api/sessions/${id}`, { method: 'DELETE' }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['sessions'] }),
  });
}

export function useResumeSession() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) =>
      fetchJson<Session>(`/api/sessions/${id}/resume`, { method: 'POST' }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['sessions'] }),
  });
}

export function useRestartSession() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) =>
      fetchJson<Session>(`/api/sessions/${id}/restart`, { method: 'POST' }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['sessions'] }),
  });
}

export function useRemoveSession() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => fetchJson(`/api/sessions/${id}/remove`, { method: 'DELETE' }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['sessions'] }),
  });
}

export function useDirsQuery(prefix: string) {
  return useQuery({
    queryKey: ['fs-dirs', prefix],
    queryFn: () =>
      fetchJson<{ dirs: string[] }>(`/api/fs/dirs?prefix=${encodeURIComponent(prefix)}`),
    enabled: prefix.length > 0,
    staleTime: 30_000,
  });
}

export function useProjectsQuery() {
  return useQuery({
    queryKey: ['projects'],
    queryFn: () => fetchJson<{ path: string; name: string }[]>('/api/projects'),
  });
}

export function useNotificationsQuery(unreadOnly = true) {
  return useQuery({
    queryKey: ['notifications', { unreadOnly }],
    queryFn: () =>
      fetchJson<NotificationMessage[]>(`/api/notifications${unreadOnly ? '?unread=true' : ''}`),
  });
}

export function useMarkNotificationRead() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: number) => fetchJson(`/api/notifications/${id}/read`, { method: 'PATCH' }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['notifications'] }),
  });
}

export function useTelegramSettingsQuery() {
  return useQuery({
    queryKey: ['telegram-settings'],
    queryFn: () =>
      fetchJson<{
        configured: boolean;
        botToken?: string;
        chatId?: string;
        dashboardUrl?: string;
        enabled?: boolean;
        events?: string[];
      }>('/api/settings/telegram'),
  });
}

export function useSaveTelegramSettings() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: {
      botToken: string;
      chatId: string;
      dashboardUrl: string;
      enabled: boolean;
      events: string[];
    }) =>
      fetchJson('/api/settings/telegram', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(input),
      }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['telegram-settings'] }),
  });
}

export function useTestTelegram() {
  return useMutation({
    mutationFn: () => fetchJson('/api/settings/telegram/test', { method: 'POST' }),
  });
}

// Pipeline hooks

export function usePipelinesQuery() {
  return useQuery({
    queryKey: ['pipelines'],
    queryFn: () => fetchJson<Pipeline[]>('/api/pipelines'),
  });
}

export function usePipelineQuery(id: string) {
  return useQuery({
    queryKey: ['pipelines', id],
    queryFn: () => fetchJson<Pipeline>(`/api/pipelines/${id}`),
    enabled: !!id,
  });
}

export function useCreatePipeline() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: CreatePipelineInputType) =>
      fetchJson<Pipeline>('/api/pipelines', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(input),
      }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['pipelines'] }),
  });
}

export function useUpdatePipeline() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, ...input }: UpdatePipelineInputType & { id: string }) =>
      fetchJson<Pipeline>(`/api/pipelines/${id}`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(input),
      }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['pipelines'] }),
  });
}

export function useDeletePipeline() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => fetchJson(`/api/pipelines/${id}`, { method: 'DELETE' }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['pipelines'] }),
  });
}
