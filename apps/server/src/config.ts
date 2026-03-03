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
  apiToken?: string;
}

export function resolveConfig(overrides?: Partial<PraefectusConfig>): PraefectusConfig {
  const home = homedir();
  const praefectusHome = overrides?.praefectusHome ?? join(home, '.praefectus');
  const configFilePath = join(praefectusHome, 'config.json');

  // Load persisted config from disk
  const fileConfig = loadConfigFile(configFilePath);

  // apiToken: overrides > config file > undefined (no auth).
  // Set apiToken in ~/.praefectus/config.json to enable auth.
  const apiToken =
    overrides !== undefined && 'apiToken' in overrides
      ? overrides?.apiToken
      : (fileConfig.apiToken as string | undefined);

  // Only merge user-facing config fields from the file — never let
  // computed path fields (dbPath, skillsDir, logsDir, pidFile, configFile)
  // be overwritten by config.json contents.
  const {
    serverPort: fileServerPort,
    webPort: fileWebPort,
    telegram,
    dashboardUrl,
    worktreeBase,
  } = fileConfig;

  return {
    praefectusHome,
    dbPath: join(praefectusHome, 'praefectus.db'),
    skillsDir: join(praefectusHome, 'skills'),
    logsDir: join(praefectusHome, 'logs'),
    pidFile: join(praefectusHome, 'server.pid'),
    configFile: configFilePath,
    worktreeBase: worktreeBase ?? join(home, 'worktrees'),
    serverPort: fileServerPort ?? 8000,
    webPort: fileWebPort ?? 3000,
    ...(telegram && { telegram }),
    ...(dashboardUrl && { dashboardUrl }),
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
