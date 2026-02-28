import pty, { type IPty } from 'node-pty';

export interface PtyCreateOptions {
  cwd: string;
  cols?: number;
  rows?: number;
  env?: Record<string, string>;
}

export class PtyManager {
  private ptys = new Map<string, IPty>();

  create(sessionId: string, cmd: string, args: string[], options: PtyCreateOptions): IPty {
    // Kill existing PTY for this session if any
    if (this.ptys.has(sessionId)) {
      this.kill(sessionId);
    }

    const ptyProcess = pty.spawn(cmd, args, {
      name: 'xterm-256color',
      cols: options.cols ?? 120,
      rows: options.rows ?? 30,
      cwd: options.cwd,
      env: (options.env as Record<string, string>) ?? (process.env as Record<string, string>),
    });

    this.ptys.set(sessionId, ptyProcess);
    return ptyProcess;
  }

  write(sessionId: string, data: string): void {
    const p = this.ptys.get(sessionId);
    if (p) {
      p.write(data);
    }
  }

  resize(sessionId: string, cols: number, rows: number): void {
    const p = this.ptys.get(sessionId);
    if (p) {
      try {
        p.resize(cols, rows);
      } catch {
        // PTY may have already exited
      }
    }
  }

  kill(sessionId: string): void {
    const p = this.ptys.get(sessionId);
    if (!p) return;

    try {
      p.kill();
    } catch {
      // Already dead
    }

    // Force kill after 5s if still alive
    const timeout = setTimeout(() => {
      try {
        p.kill('SIGKILL');
      } catch {
        // Already dead
      }
    }, 5000);

    // Clean up on exit (may fire immediately if already dead)
    p.onExit(() => {
      clearTimeout(timeout);
    });

    this.ptys.delete(sessionId);
  }

  isAlive(sessionId: string): boolean {
    return this.ptys.has(sessionId);
  }

  getActiveSessions(): string[] {
    return Array.from(this.ptys.keys());
  }

  remove(sessionId: string): void {
    this.ptys.delete(sessionId);
  }

  disposeAll(): void {
    for (const [sessionId] of this.ptys) {
      this.kill(sessionId);
    }
    this.ptys.clear();
  }
}
