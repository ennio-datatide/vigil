import { execSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import type { PraefectusConfig } from '../config.js';
import { AgentSpawner } from './agent-spawner.js';
import { EventBus } from './event-bus.js';
import { OutputManager } from './output-manager.js';
import { PtyManager } from './pty-manager.js';
import { SkillManager } from './skill-manager.js';
import { WorktreeManager } from './worktree-manager.js';

describe('AgentSpawner', () => {
  let repoDir: string;
  let worktreeBase: string;
  let skillsDir: string;
  let config: PraefectusConfig;
  let spawner: AgentSpawner;
  let worktreeManager: WorktreeManager;
  let outputManager: OutputManager;
  let ptyManager: PtyManager;

  beforeAll(() => {
    repoDir = mkdtempSync(join(tmpdir(), 'pf-spawn-test-'));
    execSync('git init && git commit --allow-empty -m "init"', { cwd: repoDir });
    worktreeBase = mkdtempSync(join(tmpdir(), 'pf-spawn-wt-'));
    skillsDir = mkdtempSync(join(tmpdir(), 'pf-spawn-skills-'));

    config = {
      praefectusHome: mkdtempSync(join(tmpdir(), 'pf-home-')),
      dbPath: '',
      skillsDir,
      logsDir: mkdtempSync(join(tmpdir(), 'pf-logs-')),
      pidFile: '',
      configFile: '',
      worktreeBase,
      serverPort: 8000,
      webPort: 3000,
    };

    worktreeManager = new WorktreeManager(worktreeBase);
    const skillManager = new SkillManager(skillsDir);
    outputManager = new OutputManager();
    ptyManager = new PtyManager();
    const eventBus = new EventBus();
    spawner = new AgentSpawner(
      worktreeManager,
      skillManager,
      config,
      outputManager,
      ptyManager,
      eventBus,
    );
  });

  afterAll(async () => {
    ptyManager.disposeAll();
    await worktreeManager.removeAll(repoDir);
  });

  it('should install hooks with correct session ID and port', async () => {
    const { worktreePath } = await worktreeManager.create(repoDir, 'hook-test');

    await spawner.installHooks(worktreePath, 'test-session-123');

    // Verify hook script exists and has correct content
    const hookPath = join(worktreePath, '.claude', 'hooks', 'emit-event.sh');
    expect(existsSync(hookPath)).toBe(true);

    const hookContent = readFileSync(hookPath, 'utf-8');
    expect(hookContent).toContain('SESSION_ID="test-session-123"');
    expect(hookContent).toContain('localhost:8000');
    expect(hookContent).not.toContain('__SESSION_ID__');
    expect(hookContent).not.toContain('__SERVER_PORT__');
  });

  it('should make hook script executable', async () => {
    const { worktreePath } = await worktreeManager.create(repoDir, 'hook-perms');

    await spawner.installHooks(worktreePath, 'perm-test');

    const hookPath = join(worktreePath, '.claude', 'hooks', 'emit-event.sh');
    const stats = statSync(hookPath);
    const isExecutable = (stats.mode & 0o111) !== 0;
    expect(isExecutable).toBe(true);
  });

  it('should install settings.json with correct hooks directory', async () => {
    const { worktreePath } = await worktreeManager.create(repoDir, 'settings-test');

    await spawner.installHooks(worktreePath, 'settings-session');

    const settingsPath = join(worktreePath, '.claude', 'settings.json');
    expect(existsSync(settingsPath)).toBe(true);

    const settings = JSON.parse(readFileSync(settingsPath, 'utf-8'));
    expect(settings.hooks).toBeDefined();
    expect(settings.hooks.PreToolUse).toBeDefined();
    expect(settings.hooks.PostToolUse).toBeDefined();
    expect(settings.hooks.Stop).toBeDefined();
    expect(settings.hooks.SubagentStart).toBeDefined();
    expect(settings.hooks.SubagentStop).toBeDefined();
    expect(settings.hooks.Notification).toBeDefined();

    // New format: each event has { hooks: [{ type, command }] }
    const hookCmd = settings.hooks.PreToolUse[0].hooks[0].command;
    expect(hookCmd).not.toContain('__HOOKS_DIR__');
    expect(hookCmd).toContain('emit-event.sh');
    expect(hookCmd).toContain(join(worktreePath, '.claude', 'hooks'));
  });

  it('should build correct claude args without skill', () => {
    const args = spawner.buildClaudeArgs('Add auth middleware');
    expect(args).toEqual([
      '-p',
      'Add auth middleware',
      '--output-format',
      'stream-json',
      '--include-partial-messages',
      '--verbose',
    ]);
  });

  it('should build correct claude args with skill', () => {
    const args = spawner.buildClaudeArgs('Review code', 'review');
    expect(args).toEqual([
      '-p',
      '/review Review code',
      '--output-format',
      'stream-json',
      '--include-partial-messages',
      '--verbose',
    ]);
  });

  it('should pass prompt with special characters safely as args', () => {
    const args = spawner.buildClaudeArgs("Don't break things; rm -rf /");
    expect(args).toEqual([
      '-p',
      "Don't break things; rm -rf /",
      '--output-format',
      'stream-json',
      '--include-partial-messages',
      '--verbose',
    ]);
  });

  it('should report session as not alive when no process exists', () => {
    const alive = spawner.isAlive('nonexistent');
    expect(alive).toBe(false);
  });

  it('should not throw when killing a non-existent session', async () => {
    await expect(spawner.kill('nonexistent')).resolves.not.toThrow();
  });

  describe('captureGitMetadata', () => {
    it('should capture git metadata from a git repo', () => {
      const meta = spawner.captureGitMetadata(repoDir);
      expect(meta).not.toBeNull();
      expect(meta?.branch).toBe('master');
      expect(meta?.commitHash).toMatch(/^[a-f0-9]+$/);
      expect(meta?.repoName).toBeDefined();
    });

    it('should return null for a non-git directory', () => {
      const tmpDir = mkdtempSync(join(tmpdir(), 'not-git-'));
      const meta = spawner.captureGitMetadata(tmpDir);
      expect(meta).toBeNull();
    });

    it('should handle missing remote gracefully', () => {
      const meta = spawner.captureGitMetadata(repoDir);
      expect(meta).not.toBeNull();
      // Our test repo has no remote, so remoteUrl should be null
      expect(meta?.remoteUrl).toBeNull();
    });
  });
});
