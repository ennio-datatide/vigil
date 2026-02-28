import { DEFAULT_SERVER_PORT } from '@praefectus/shared';

export async function start(
  project: string,
  prompt: string,
  options: { skill?: string; role?: string },
) {
  const baseUrl = `http://localhost:${DEFAULT_SERVER_PORT}`;

  try {
    const body: Record<string, string> = { projectPath: project, prompt };
    if (options.skill) body.skill = options.skill;
    if (options.role) body.role = options.role;

    const res = await fetch(`${baseUrl}/api/sessions`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });

    if (!res.ok) {
      const err = await res.json().catch(() => ({ error: res.statusText }));
      console.error(
        `Failed to start session: ${(err as { error?: string }).error ?? res.statusText}`,
      );
      process.exit(1);
    }

    const session = (await res.json()) as { id: string; status: string };
    console.log(`Session started: ${session.id} (${session.status})`);
  } catch (_err) {
    console.error('Could not reach Praefectus server. Is it running? Try: praefectus up');
    process.exit(1);
  }
}
