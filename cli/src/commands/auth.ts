import { spawn } from 'node:child_process';
import { DEFAULT_SERVER_PORT } from '@praefectus/shared';

export async function auth(action: string) {
  switch (action) {
    case 'status':
      await checkAuthStatus();
      break;
    case 'claude':
      runAuthDirect('claude');
      break;
    case 'codex':
      runAuthDirect('codex');
      break;
    default:
      console.error(`Unknown auth action: ${action}. Use: status, claude, codex`);
      process.exit(1);
  }
}

async function checkAuthStatus() {
  const baseUrl = `http://localhost:${DEFAULT_SERVER_PORT}`;

  try {
    const res = await fetch(`${baseUrl}/health`);

    if (!res.ok) {
      console.error(`Server returned: ${res.statusText}`);
      process.exit(1);
    }

    const data = (await res.json()) as { status: string };
    console.log(`Server: ${data.status}`);
    console.log('Authentication status: connected');
  } catch {
    console.error('Could not reach Praefectus server. Is it running? Try: praefectus up');
    process.exit(1);
  }
}

/** Run the tool directly in the user's terminal for authentication (no tmux). */
function runAuthDirect(tool: string) {
  console.log(`Launching ${tool} for authentication...`);
  console.log('Complete the login flow, then exit with /exit or Ctrl+C.\n');

  const child = spawn(tool, [], {
    stdio: 'inherit',
    env: { ...process.env, TMUX: undefined }, // Clear TMUX env in case we're inside one
  });

  child.on('close', (code) => {
    if (code === 0) {
      console.log(`\n${tool} authentication complete.`);
    }
    process.exit(code ?? 0);
  });

  child.on('error', (err) => {
    if ((err as NodeJS.ErrnoException).code === 'ENOENT') {
      console.error(`${tool} is not installed or not in PATH.`);
    } else {
      console.error(`Failed to launch ${tool}: ${err.message}`);
    }
    process.exit(1);
  });
}
