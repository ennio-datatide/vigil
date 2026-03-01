import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { loadConfigFile, resolveConfig } from './config.js';

describe('config', () => {
  const tmpDir = `/tmp/pf-config-test-${Date.now()}`;

  beforeEach(() => {
    mkdirSync(tmpDir, { recursive: true });
  });

  afterEach(() => {
    rmSync(tmpDir, { recursive: true, force: true });
  });

  describe('resolveConfig', () => {
    it('should derive computed paths from praefectusHome', () => {
      const config = resolveConfig({ praefectusHome: tmpDir });
      expect(config.praefectusHome).toBe(tmpDir);
      expect(config.dbPath).toBe(join(tmpDir, 'praefectus.db'));
      expect(config.skillsDir).toBe(join(tmpDir, 'skills'));
      expect(config.logsDir).toBe(join(tmpDir, 'logs'));
      expect(config.pidFile).toBe(join(tmpDir, 'server.pid'));
      expect(config.configFile).toBe(join(tmpDir, 'config.json'));
    });

    it('should respect praefectusHome override', () => {
      const customHome = `${tmpDir}/custom`;
      mkdirSync(customHome, { recursive: true });
      const config = resolveConfig({ praefectusHome: customHome });
      expect(config.praefectusHome).toBe(customHome);
      expect(config.dbPath).toBe(join(customHome, 'praefectus.db'));
      expect(config.skillsDir).toBe(join(customHome, 'skills'));
      expect(config.logsDir).toBe(join(customHome, 'logs'));
    });

    it('should not let config file clobber computed path fields', () => {
      const configFile = join(tmpDir, 'config.json');
      writeFileSync(
        configFile,
        JSON.stringify({
          dbPath: '/evil/db.sqlite',
          skillsDir: '/evil/skills',
          logsDir: '/evil/logs',
          pidFile: '/evil/server.pid',
          configFile: '/evil/config.json',
          serverPort: 5555,
        }),
      );

      const config = resolveConfig({ praefectusHome: tmpDir });

      // Computed paths must always be derived from praefectusHome
      expect(config.dbPath).toBe(join(tmpDir, 'praefectus.db'));
      expect(config.skillsDir).toBe(join(tmpDir, 'skills'));
      expect(config.logsDir).toBe(join(tmpDir, 'logs'));
      expect(config.pidFile).toBe(join(tmpDir, 'server.pid'));
      expect(config.configFile).toBe(join(tmpDir, 'config.json'));

      // User-facing fields should still be read from config file
      expect(config.serverPort).toBe(5555);
    });

    it('should auto-generate apiToken and persist to config file when missing', () => {
      const config = resolveConfig({ praefectusHome: tmpDir });
      expect(config.apiToken).toBeDefined();
      expect(config.apiToken).toHaveLength(64); // 32 bytes = 64 hex chars

      // Token should be persisted to config.json
      const configFile = join(tmpDir, 'config.json');
      expect(existsSync(configFile)).toBe(true);
      const saved = JSON.parse(readFileSync(configFile, 'utf-8'));
      expect(saved.apiToken).toBe(config.apiToken);
    });

    it('should reuse existing apiToken from config file', () => {
      const configFile = join(tmpDir, 'config.json');
      writeFileSync(configFile, JSON.stringify({ apiToken: 'my-custom-token' }));

      const config = resolveConfig({ praefectusHome: tmpDir });
      expect(config.apiToken).toBe('my-custom-token');
    });
  });

  describe('loadConfigFile', () => {
    it('should load existing config', () => {
      const configFile = join(tmpDir, 'test-config.json');
      writeFileSync(configFile, JSON.stringify({ serverPort: 9999 }));
      const loaded = loadConfigFile(configFile);
      expect(loaded.serverPort).toBe(9999);
    });

    it('should return empty object for missing file', () => {
      const loaded = loadConfigFile(join(tmpDir, 'nonexistent.json'));
      expect(loaded).toEqual({});
    });
  });
});
