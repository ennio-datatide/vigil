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
