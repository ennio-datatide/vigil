import { DEFAULT_SERVER_PORT } from '@praefectus/shared';

interface SessionRow {
  id: string;
  status: string;
  projectPath: string;
  prompt: string;
}

export async function ls(options: { all?: boolean }) {
  const baseUrl = `http://localhost:${DEFAULT_SERVER_PORT}`;

  try {
    const res = await fetch(`${baseUrl}/api/sessions`);

    if (!res.ok) {
      console.error(`Failed to list sessions: ${res.statusText}`);
      process.exit(1);
    }

    const sessions = (await res.json()) as SessionRow[];

    // Filter out completed unless --all
    const filtered = options.all
      ? sessions
      : sessions.filter((s) => !['completed', 'cancelled', 'error'].includes(s.status));

    if (filtered.length === 0) {
      console.log(options.all ? 'No sessions found' : 'No active sessions. Use --all to see completed.');
      return;
    }

    // Simple table output
    const idWidth = 14;
    const statusWidth = 12;
    const projectWidth = 30;
    const promptWidth = 40;

    const header = [
      'ID'.padEnd(idWidth),
      'STATUS'.padEnd(statusWidth),
      'PROJECT'.padEnd(projectWidth),
      'PROMPT'.padEnd(promptWidth),
    ].join('  ');

    console.log(header);
    console.log('-'.repeat(header.length));

    for (const s of filtered) {
      const truncatedPrompt = s.prompt.length > promptWidth
        ? s.prompt.slice(0, promptWidth - 3) + '...'
        : s.prompt;
      const truncatedProject = s.projectPath.length > projectWidth
        ? '...' + s.projectPath.slice(-(projectWidth - 3))
        : s.projectPath;

      console.log([
        s.id.padEnd(idWidth),
        s.status.padEnd(statusWidth),
        truncatedProject.padEnd(projectWidth),
        truncatedPrompt.padEnd(promptWidth),
      ].join('  '));
    }
  } catch {
    console.error('Could not reach Praefectus server. Is it running? Try: praefectus up');
    process.exit(1);
  }
}
