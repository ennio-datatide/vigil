import { readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';

export interface PraefectusConfig {
  praefectusHome: string;
  dbPath: string;
  skillsDir: string;
  logsDir: string;
  pidFile: string;
  configFile: string;
  worktreeBase: string;
  serverPort: number;
  webPort: number;
  telegram?: {
    botToken: string;
    chatId: string;
  };
  dashboardUrl?: string;
}

export function resolveConfig(overrides?: Partial<PraefectusConfig>): PraefectusConfig {
  const home = homedir();
  const praefectusHome = overrides?.praefectusHome ?? join(home, '.praefectus');

  return {
    praefectusHome,
    dbPath: join(praefectusHome, 'praefectus.db'),
    skillsDir: join(praefectusHome, 'skills'),
    logsDir: join(praefectusHome, 'logs'),
    pidFile: join(praefectusHome, 'server.pid'),
    configFile: join(praefectusHome, 'config.json'),
    worktreeBase: join(home, 'worktrees'),
    serverPort: 8000,
    webPort: 3000,
    ...overrides,
  };
}

export function loadConfigFile(configPath: string): Partial<PraefectusConfig> {
  try {
    return JSON.parse(readFileSync(configPath, 'utf-8'));
  } catch {
    return {};
  }
}
