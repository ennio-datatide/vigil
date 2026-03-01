import { randomBytes } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
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
  apiToken?: string;
}

export function resolveConfig(overrides?: Partial<PraefectusConfig>): PraefectusConfig {
  const home = homedir();
  const praefectusHome = overrides?.praefectusHome ?? join(home, '.praefectus');
  const configFilePath = join(praefectusHome, 'config.json');

  // Load persisted config from disk
  const fileConfig = loadConfigFile(configFilePath);

  // Determine apiToken: overrides > config file > auto-generate
  let apiToken = overrides?.apiToken ?? fileConfig.apiToken;
  let tokenGenerated = false;
  if (!apiToken) {
    apiToken = randomBytes(32).toString('hex');
    tokenGenerated = true;
  }

  // Persist newly generated token to config.json
  if (tokenGenerated) {
    mkdirSync(praefectusHome, { recursive: true });
    const toSave = { ...fileConfig, apiToken };
    writeFileSync(configFilePath, JSON.stringify(toSave, null, 2));
  }

  return {
    praefectusHome,
    dbPath: join(praefectusHome, 'praefectus.db'),
    skillsDir: join(praefectusHome, 'skills'),
    logsDir: join(praefectusHome, 'logs'),
    pidFile: join(praefectusHome, 'server.pid'),
    configFile: configFilePath,
    worktreeBase: join(home, 'worktrees'),
    serverPort: 8000,
    webPort: 3000,
    ...fileConfig,
    ...overrides,
    apiToken,
  };
}

export function loadConfigFile(configPath: string): Partial<PraefectusConfig> {
  try {
    return JSON.parse(readFileSync(configPath, 'utf-8'));
  } catch {
    return {};
  }
}
