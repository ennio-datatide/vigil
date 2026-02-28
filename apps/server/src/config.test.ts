import { describe, expect, it } from 'vitest';
import { resolveConfig } from './config.js';

describe('config', () => {
  it('should resolve praefectus home directory', () => {
    const config = resolveConfig();
    expect(config.praefectusHome).toMatch(/\.praefectus$/);
    expect(config.dbPath).toMatch(/praefectus\.db$/);
    expect(config.skillsDir).toMatch(/skills$/);
    expect(config.serverPort).toBe(8000);
  });

  it('should respect PRAEFECTUS_HOME env override', () => {
    const config = resolveConfig({ praefectusHome: '/tmp/test-praefectus' });
    expect(config.praefectusHome).toBe('/tmp/test-praefectus');
    expect(config.dbPath).toBe('/tmp/test-praefectus/praefectus.db');
  });
});
