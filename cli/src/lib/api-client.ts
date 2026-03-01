import { readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { DEFAULT_SERVER_PORT } from '@praefectus/shared';

const BASE_URL = `http://localhost:${DEFAULT_SERVER_PORT}`;

function loadToken(): string | null {
  try {
    const configPath = join(homedir(), '.praefectus', 'config.json');
    const config = JSON.parse(readFileSync(configPath, 'utf-8'));
    return config.apiToken ?? null;
  } catch {
    return null;
  }
}

export function getBaseUrl(): string {
  return BASE_URL;
}

export function authHeaders(): Record<string, string> {
  const token = loadToken();
  if (!token) return {};
  return { Authorization: `Bearer ${token}` };
}

export async function apiFetch(path: string, init?: RequestInit): Promise<Response> {
  const headers = { ...authHeaders(), ...Object.fromEntries(new Headers(init?.headers).entries()) };
  return fetch(`${BASE_URL}${path}`, { ...init, headers });
}
