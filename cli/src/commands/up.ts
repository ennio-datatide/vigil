import { spawn } from 'node:child_process';
import { writeFileSync, readFileSync, existsSync, mkdirSync, unlinkSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { homedir } from 'node:os';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

function isProcessRunning(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

export async function up(options: { daemon?: boolean }) {
  const praefectusHome = join(homedir(), '.praefectus');
  const pidFile = join(praefectusHome, 'server.pid');

  // Check if already running
  if (existsSync(pidFile)) {
    const pid = parseInt(readFileSync(pidFile, 'utf-8').trim(), 10);
    if (isProcessRunning(pid)) {
      console.log(`Praefectus is already running (PID ${pid})`);
      return;
    }
    // Stale PID file, clean up
  }

  mkdirSync(praefectusHome, { recursive: true });
  mkdirSync(join(praefectusHome, 'logs'), { recursive: true });

  console.log('Starting Praefectus...');

  // Start server and web via turbo from monorepo root
  const monorepoRoot = join(__dirname, '..', '..', '..');
  const child = spawn('npx', ['turbo', 'dev', '--filter=@praefectus/server', '--filter=@praefectus/web'], {
    cwd: monorepoRoot,
    detached: options.daemon,
    stdio: options.daemon ? 'ignore' : 'inherit',
  });

  if (options.daemon) {
    writeFileSync(pidFile, String(child.pid));
    child.unref();
    console.log(`Praefectus running in background (PID ${child.pid})`);
    console.log('Dashboard: http://localhost:3000');
  } else {
    child.on('close', (code) => {
      try { unlinkSync(pidFile); } catch { /* ignore */ }
      process.exit(code ?? 0);
    });
    writeFileSync(pidFile, String(child.pid));
    console.log('Dashboard: http://localhost:3000');
  }
}
