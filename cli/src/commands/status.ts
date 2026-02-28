import { DEFAULT_SERVER_PORT } from '@praefectus/shared';

interface SessionRow {
  id: string;
  status: string;
}

export async function status() {
  const baseUrl = `http://localhost:${DEFAULT_SERVER_PORT}`;

  try {
    // Fetch health and sessions in parallel
    const [healthRes, sessionsRes] = await Promise.all([
      fetch(`${baseUrl}/health`),
      fetch(`${baseUrl}/api/sessions`),
    ]);

    if (!healthRes.ok) {
      console.error(`Server health check failed: ${healthRes.statusText}`);
      process.exit(1);
    }

    const health = (await healthRes.json()) as { status: string };
    console.log(`Server: ${health.status} (port ${DEFAULT_SERVER_PORT})`);

    if (sessionsRes.ok) {
      const sessions = (await sessionsRes.json()) as SessionRow[];

      const counts: Record<string, number> = {};
      for (const s of sessions) {
        counts[s.status] = (counts[s.status] ?? 0) + 1;
      }

      console.log(`\nSessions (${sessions.length} total):`);
      if (Object.keys(counts).length === 0) {
        console.log('  No sessions');
      } else {
        for (const [st, count] of Object.entries(counts)) {
          console.log(`  ${st}: ${count}`);
        }
      }
    }
  } catch {
    console.error('Could not reach Praefectus server. Is it running? Try: praefectus up');
    process.exit(1);
  }
}
