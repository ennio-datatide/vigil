import { existsSync, readFileSync, unlinkSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';

export async function down() {
  const pidFile = join(homedir(), '.praefectus', 'server.pid');

  if (!existsSync(pidFile)) {
    console.log('Praefectus is not running');
    return;
  }

  const pid = parseInt(readFileSync(pidFile, 'utf-8').trim(), 10);

  try {
    // Kill the process group (negative pid sends signal to the group)
    process.kill(-pid, 'SIGTERM');
    console.log('Praefectus stopped');
  } catch {
    console.log('Praefectus process not found, cleaning up');
  }

  unlinkSync(pidFile);
}
