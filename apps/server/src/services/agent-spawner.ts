import { spawn, execSync, type ChildProcess } from 'node:child_process';
import { join, dirname } from 'node:path';
import { mkdirSync, writeFileSync, readFileSync, chmodSync, existsSync, createWriteStream } from 'node:fs';
import { fileURLToPath } from 'node:url';
import type { PraefectusConfig } from '../config.js';
import { WorktreeManager } from './worktree-manager.js';
import { SkillManager } from './skill-manager.js';
import type { OutputManager } from './output-manager.js';
import type { PtyManager } from './pty-manager.js';
import type { EventBus } from './event-bus.js';

export interface GitMetadata {
  repoName: string;
  branch: string;
  commitHash: string;
  remoteUrl: string | null;
}

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

/**
 * Extract human-readable text from a stream-json JSONL line.
 * With --include-partial-messages, we get incremental stream_event deltas.
 * Returns the text to display in the terminal, or null to skip the line.
 */
function extractDisplayText(jsonLine: string): string | null {
  try {
    const event = JSON.parse(jsonLine);

    // Incremental text delta — token-by-token streaming
    if (event.type === 'stream_event' && event.event?.type === 'content_block_delta') {
      const delta = event.event.delta;
      if (delta?.type === 'text_delta' && delta.text) {
        return delta.text;
      }
    }

    // Tool use start — show what tool is being called
    if (event.type === 'stream_event' && event.event?.type === 'content_block_start') {
      const block = event.event.content_block;
      if (block?.type === 'tool_use') {
        return `\r\n[Tool: ${block.name}]\r\n`;
      }
    }

    // Tool result
    if (event.type === 'tool' && event.content) {
      const textParts = event.content
        .filter((c: { type: string }) => c.type === 'text')
        .map((c: { text: string }) => c.text);
      if (textParts.length > 0) {
        const text = textParts.join('');
        const display = text.length > 500 ? text.substring(0, 500) + '...' : text;
        return `${display}\r\n`;
      }
    }

    // Skip assistant (full message), result (duplicate), system, and other event types
    return null;
  } catch {
    // Not valid JSON — pass through as-is
    return jsonLine;
  }
}

export class AgentSpawner {
  private processes = new Map<string, ChildProcess>();

  constructor(
    private worktreeManager: WorktreeManager,
    private skillManager: SkillManager,
    private config: PraefectusConfig,
    private outputManager: OutputManager,
    private ptyManager: PtyManager,
    private eventBus: EventBus,
    private onExit?: (sessionId: string, code: number | null) => void,
  ) {}

  async spawn(params: {
    sessionId: string;
    projectPath: string;
    prompt: string;
    skill?: string;
  }): Promise<{ worktreePath: string; pid: number }> {
    const { sessionId, projectPath, prompt, skill } = params;

    // 1. Try to create a worktree for isolation; fall back to project dir if not a git repo
    let cwd: string;
    const isGitRepo = existsSync(join(projectPath, '.git'));
    if (isGitRepo) {
      try {
        const { worktreePath } = await this.worktreeManager.create(projectPath, sessionId);
        cwd = worktreePath;
      } catch {
        cwd = projectPath;
      }
    } else {
      cwd = projectPath;
    }

    // 2. Install hooks
    await this.installHooks(cwd, sessionId);

    // 3. Symlink skills
    await this.skillManager.installSkills(cwd);

    // 4. Build args and spawn child process
    const args = this.buildClaudeArgs(prompt, skill);
    const logPath = join(this.config.logsDir, `${sessionId}.log`);
    const logStream = createWriteStream(logPath, { flags: 'a' });

    this.outputManager.createBuffer(sessionId);

    // Clear env vars that prevent nesting
    const env = { ...process.env };
    delete env.TMUX;
    delete env.CLAUDECODE;

    const child = spawn('claude', args, {
      cwd,
      env,
      stdio: ['ignore', 'pipe', 'pipe'],
    });

    this.processes.set(sessionId, child);

    // Buffer partial lines from stdout (JSONL may arrive in chunks)
    let stdoutBuffer = '';

    child.stdout?.on('data', (chunk: Buffer) => {
      stdoutBuffer += chunk.toString();

      // Process complete lines
      let newlineIdx: number;
      while ((newlineIdx = stdoutBuffer.indexOf('\n')) !== -1) {
        const line = stdoutBuffer.substring(0, newlineIdx).trim();
        stdoutBuffer = stdoutBuffer.substring(newlineIdx + 1);

        if (!line) continue;

        const displayText = extractDisplayText(line);
        if (displayText) {
          logStream.write(displayText);
          this.outputManager.append(sessionId, displayText);
        }
      }
    });

    child.stderr?.on('data', (chunk: Buffer) => {
      const text = chunk.toString();
      logStream.write(text);
      this.outputManager.append(sessionId, text);
    });

    child.on('close', (code) => {
      // Flush any remaining buffer
      if (stdoutBuffer.trim()) {
        const displayText = extractDisplayText(stdoutBuffer.trim());
        if (displayText) {
          logStream.write(displayText);
          this.outputManager.append(sessionId, displayText);
        }
      }
      logStream.end();
      this.processes.delete(sessionId);
      this.onExit?.(sessionId, code);
    });

    child.on('error', (err) => {
      logStream.write(`\nProcess error: ${err.message}\n`);
      logStream.end();
      this.processes.delete(sessionId);
      this.onExit?.(sessionId, 1);
    });

    return { worktreePath: cwd, pid: child.pid! };
  }

  /** Build args array for claude -p with streaming JSON output. */
  buildClaudeArgs(prompt: string, skill?: string): string[] {
    const fullPrompt = skill ? `/${skill} ${prompt}` : prompt;
    return ['-p', fullPrompt, '--output-format', 'stream-json', '--include-partial-messages', '--verbose'];
  }

  async spawnInteractive(params: {
    sessionId: string;
    projectPath: string;
    prompt?: string;
    continueInWorktree?: string;
    skipPermissions?: boolean;
    cols?: number;
    rows?: number;
  }): Promise<{ worktreePath: string; gitMetadata: GitMetadata | null }> {
    const { sessionId, projectPath, prompt, continueInWorktree, skipPermissions, cols, rows } = params;

    try {
      // 1. Determine cwd: reuse worktree for resume, create new for fresh session
      let cwd: string;
      if (continueInWorktree && existsSync(continueInWorktree)) {
        cwd = continueInWorktree;
      } else {
        const isGitRepo = existsSync(join(projectPath, '.git'));
        if (isGitRepo) {
          try {
            const { worktreePath } = await this.worktreeManager.create(projectPath, sessionId);
            cwd = worktreePath;
          } catch {
            cwd = projectPath;
          }
        } else {
          cwd = projectPath;
        }
      }

      // 2. Install hooks + skills
      await this.installHooks(cwd, sessionId);
      await this.skillManager.installSkills(cwd);

      // 3. Capture git metadata
      const gitMetadata = this.captureGitMetadata(projectPath);

      // 4. Build args: --continue for resume, --verbose for fresh
      const args: string[] = continueInWorktree
        ? ['--continue', '--verbose']
        : ['--verbose'];

      if (skipPermissions) {
        args.push('--dangerously-skip-permissions');
      }

      // 5. Set up log file
      const logPath = join(this.config.logsDir, `${sessionId}.log`);
      const logStream = createWriteStream(logPath, { flags: 'a' });

      this.outputManager.createBuffer(sessionId);

      // 6. Build env — clear vars that prevent nesting
      const env: Record<string, string> = {};
      for (const [k, v] of Object.entries(process.env)) {
        if (v !== undefined) env[k] = v;
      }
      delete env.TMUX;
      delete env.CLAUDECODE;
      env.TERM = 'xterm-256color';

      // 7. Spawn PTY
      const ptyProcess = this.ptyManager.create(sessionId, 'claude', args, {
        cwd,
        cols: cols ?? 120,
        rows: rows ?? 30,
        env,
      });

      // 8. Wire output: PTY → OutputManager + log file
      //    Auto-handle Claude's trust prompt, settings errors, and prompt delivery
      let readyForPrompt = false;
      let promptSent = false;

      // Fallback: if Claude's UI readiness isn't detected within 15s, force-send prompt
      const readyFallback = prompt && !continueInWorktree
        ? setTimeout(() => {
            if (!promptSent && this.ptyManager.isAlive(sessionId)) {
              readyForPrompt = true;
              this.sendPromptToPty(sessionId, prompt);
              promptSent = true;
            }
          }, 15_000)
        : null;

      ptyProcess.onData((data: string) => {
        logStream.write(data);
        this.outputManager.append(sessionId, data);

        // Auto-accept the workspace trust prompt by sending Enter
        // The trust prompt contains "trust" in the ANSI output
        if (!readyForPrompt && data.includes('trust')) {
          setTimeout(() => {
            if (this.ptyManager.isAlive(sessionId)) {
              this.ptyManager.write(sessionId, '\r');
            }
          }, 500);
        }

        // Auto-dismiss settings error prompts with Enter
        if (!promptSent && data.includes('SettingsError')) {
          setTimeout(() => {
            if (this.ptyManager.isAlive(sessionId)) {
              this.ptyManager.write(sessionId, '\r');
            }
          }, 1000);
        }

        // Detect when Claude's main input area is ready
        // The status bar shows "tokens" and/or "bypass" when the TUI is accepting input
        if (!readyForPrompt && (data.includes('tokens') || data.includes('bypass'))) {
          readyForPrompt = true;
        }

        // Send the prompt once Claude's input area is ready
        if (prompt && !continueInWorktree && readyForPrompt && !promptSent) {
          promptSent = true;
          if (readyFallback) clearTimeout(readyFallback);
          // Small delay to let the UI fully stabilize before typing
          setTimeout(() => {
            this.sendPromptToPty(sessionId, prompt);
          }, 1000);
        }
      });

      ptyProcess.onExit(({ exitCode }) => {
        // Notify terminal viewers that the agent process has exited
        // exitCode may be null (e.g. killed by signal) — treat as non-zero
        const code = exitCode ?? 1;
        const exitMsg = code === 0
          ? '\r\n\x1b[33m[Agent exited normally]\x1b[0m\r\n'
          : `\r\n\x1b[31m[Agent exited with code ${code}]\x1b[0m\r\n`;
        this.outputManager.append(sessionId, exitMsg);
        logStream.write(exitMsg);
        logStream.end();
        this.ptyManager.remove(sessionId);
        this.onExit?.(sessionId, code);
      });

      // Emit spawn success event
      this.eventBus.emit('session_spawned', {
        sessionId,
        worktreePath: cwd,
        gitMetadata: gitMetadata ? JSON.stringify(gitMetadata) : null,
      });

      return { worktreePath: cwd, gitMetadata };
    } catch (err) {
      // Emit spawn failure event
      this.eventBus.emit('session_spawn_failed', {
        sessionId,
        error: err instanceof Error ? err.message : String(err),
      });
      throw err;
    }
  }

  captureGitMetadata(projectPath: string): GitMetadata | null {
    try {
      const exec = (cmd: string) =>
        execSync(cmd, { cwd: projectPath, timeout: 5000 }).toString().trim();

      return {
        repoName: exec('git rev-parse --show-toplevel').split('/').pop()!,
        branch: exec('git rev-parse --abbrev-ref HEAD'),
        commitHash: exec('git rev-parse --short HEAD'),
        remoteUrl: (() => {
          try { return exec('git remote get-url origin'); }
          catch { return null; }
        })(),
      };
    } catch {
      return null;
    }
  }

  /** Write prompt text + Enter to a PTY session (Claude's TUI needs them as distinct writes). */
  private sendPromptToPty(sessionId: string, prompt: string): void {
    if (!this.ptyManager.isAlive(sessionId)) return;
    this.ptyManager.write(sessionId, prompt);
    setTimeout(() => {
      if (this.ptyManager.isAlive(sessionId)) {
        this.ptyManager.write(sessionId, '\r');
      }
    }, 500);
  }

  async kill(sessionId: string): Promise<void> {
    // Try PTY first
    if (this.ptyManager.isAlive(sessionId)) {
      this.ptyManager.kill(sessionId);
      return;
    }

    // Fall back to child process
    const child = this.processes.get(sessionId);
    if (!child) return;

    child.kill('SIGTERM');

    const timeout = setTimeout(() => {
      if (!child.killed) {
        child.kill('SIGKILL');
      }
    }, 5000);

    child.on('close', () => {
      clearTimeout(timeout);
    });
  }

  isAlive(sessionId: string): boolean {
    return this.processes.has(sessionId) || this.ptyManager.isAlive(sessionId);
  }

  async installHooks(targetDir: string, sessionId: string): Promise<void> {
    const claudeDir = join(targetDir, '.claude');
    const hooksDir = join(claudeDir, 'hooks');
    mkdirSync(hooksDir, { recursive: true });

    // Copy and configure emit-event.sh
    const hookTemplatePath = join(__dirname, '..', 'hooks', 'emit-event.sh');
    const hookTemplate = readFileSync(hookTemplatePath, 'utf-8');
    const hookContent = hookTemplate
      .replace(/__SESSION_ID__/g, sessionId)
      .replace(/__SERVER_PORT__/g, String(this.config.serverPort));

    const hookPath = join(hooksDir, 'emit-event.sh');
    writeFileSync(hookPath, hookContent);
    chmodSync(hookPath, 0o755);

    // Copy and configure settings.json
    const settingsTemplatePath = join(__dirname, '..', 'hooks', 'templates', 'settings.json');
    const settingsTemplate = readFileSync(settingsTemplatePath, 'utf-8');
    const settingsContent = settingsTemplate.replace(/__HOOKS_DIR__/g, hooksDir);

    writeFileSync(join(claudeDir, 'settings.json'), settingsContent);
  }
}
