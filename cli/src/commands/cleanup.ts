import { apiFetch } from '../lib/api-client.js';

export async function cleanup() {
  try {
    const res = await apiFetch('/api/cleanup', {
      method: 'POST',
    });
    const data = (await res.json()) as { removed: number; skipped: number };
    console.log(`Cleanup complete: ${data.removed} removed, ${data.skipped} skipped`);
  } catch {
    console.error('Failed to connect to server. Is it running?');
  }
}
