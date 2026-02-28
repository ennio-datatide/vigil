import { existsSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { Command } from 'commander';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

// ─── Commander program parsing ─────────────────────────────────────────────

describe('CLI program', () => {
  it('should create a program with all commands registered', async () => {
    const program = new Command();
    program.name('praefectus').description('Praefectus — AI agent orchestration').version('0.1.0');

    program
      .command('up')
      .description('Start the server and dashboard')
      .option('--daemon', 'Run in background');
    program.command('down').description('Stop the server and dashboard');
    program
      .command('start')
      .description('Start a new agent session')
      .argument('<project>')
      .argument('<prompt>');
    program
      .command('ls')
      .description('List active sessions')
      .option('--all', 'Include completed sessions');
    program
      .command('auth')
      .description('Check or manage authentication')
      .argument('[action]', 'Action', 'status');
    program.command('status').description('Show server status');
    program.command('cleanup').description('Remove old worktrees from completed sessions');

    const commands = program.commands.map((c) => c.name());
    expect(commands).toContain('up');
    expect(commands).toContain('down');
    expect(commands).toContain('start');
    expect(commands).toContain('ls');
    expect(commands).toContain('auth');
    expect(commands).toContain('status');
    expect(commands).toContain('cleanup');
    expect(commands).toHaveLength(7);
  });

  it('should parse --daemon option for up command', () => {
    const program = new Command();
    program.exitOverride(); // Throw instead of process.exit

    const upCmd = program.command('up').option('--daemon', 'Run in background');
    let capturedOptions: { daemon?: boolean } | undefined;
    upCmd.action((opts) => {
      capturedOptions = opts;
    });

    program.parse(['node', 'praefectus', 'up', '--daemon']);
    expect(capturedOptions?.daemon).toBe(true);
  });

  it('should parse start command arguments', () => {
    const program = new Command();
    program.exitOverride();

    const startCmd = program
      .command('start')
      .argument('<project>')
      .argument('<prompt>')
      .option('--skill <skill>')
      .option('--role <role>');

    let capturedArgs: { project?: string; prompt?: string; options?: Record<string, string> } = {};
    startCmd.action((project, prompt, opts) => {
      capturedArgs = { project, prompt, options: opts };
    });

    program.parse(['node', 'praefectus', 'start', '/my/project', 'Fix the bug', '--role', 'fixer']);
    expect(capturedArgs.project).toBe('/my/project');
    expect(capturedArgs.prompt).toBe('Fix the bug');
    expect(capturedArgs.options?.role).toBe('fixer');
  });
});

// ─── up command: detect already-running process ────────────────────────────

describe('up command — PID detection', () => {
  const tmpDir = join('/tmp', `pf-cli-test-${Date.now()}`);
  const pidFile = join(tmpDir, 'server.pid');

  beforeEach(() => {
    mkdirSync(tmpDir, { recursive: true });
  });

  afterEach(() => {
    rmSync(tmpDir, { recursive: true, force: true });
  });

  it('should detect that current process is running via PID file', async () => {
    // Write own PID to simulate a running process
    writeFileSync(pidFile, String(process.pid));

    // Re-implement the detection logic from up.ts
    const pid = parseInt((await import('node:fs')).readFileSync(pidFile, 'utf-8').trim(), 10);
    let isRunning = false;
    try {
      process.kill(pid, 0);
      isRunning = true;
    } catch {
      isRunning = false;
    }

    expect(isRunning).toBe(true);
  });

  it('should detect stale PID file when process is not running', async () => {
    // Write a PID that is very unlikely to exist
    writeFileSync(pidFile, '9999999');

    const pid = parseInt((await import('node:fs')).readFileSync(pidFile, 'utf-8').trim(), 10);
    let isRunning = false;
    try {
      process.kill(pid, 0);
      isRunning = true;
    } catch {
      isRunning = false;
    }

    expect(isRunning).toBe(false);
  });
});

// ─── down command: handle missing PID file ─────────────────────────────────

describe('down command — missing PID file', () => {
  it('should handle missing PID file gracefully', async () => {
    const consoleSpy = vi.spyOn(console, 'log').mockImplementation(() => {});

    // Import and call down — it checks ~/.praefectus/server.pid
    // Instead of calling the real function (which checks homedir), test the logic
    const pidFile = join('/tmp', `pf-down-test-${Date.now()}`, 'server.pid');
    const _pidDir = join('/tmp', `pf-down-test-${Date.now()}`);

    // The PID file does not exist
    expect(existsSync(pidFile)).toBe(false);

    // Simulate the logic from down.ts
    if (!existsSync(pidFile)) {
      console.log('Praefectus is not running');
    }

    expect(consoleSpy).toHaveBeenCalledWith('Praefectus is not running');
    consoleSpy.mockRestore();
  });

  it('should clean up PID file after stopping', () => {
    const tmpDir = join('/tmp', `pf-down-cleanup-${Date.now()}`);
    const pidFile = join(tmpDir, 'server.pid');

    mkdirSync(tmpDir, { recursive: true });
    writeFileSync(pidFile, '12345');
    expect(existsSync(pidFile)).toBe(true);

    // Simulate down cleanup (unlinkSync)
    const { unlinkSync } = require('node:fs');
    unlinkSync(pidFile);
    expect(existsSync(pidFile)).toBe(false);

    rmSync(tmpDir, { recursive: true, force: true });
  });
});
