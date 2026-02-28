import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { beforeEach, describe, expect, it } from 'vitest';
import { createDb, initializeSchema } from '../db/client.js';
import { SettingsService } from './settings-service.js';

describe('SettingsService', () => {
  let service: SettingsService;

  beforeEach(() => {
    const dir = mkdtempSync(join(tmpdir(), 'settings-test-'));
    const { sqlite, db } = createDb(join(dir, 'test.db'));
    initializeSchema(sqlite);
    service = new SettingsService(db);
  });

  it('should return null for missing key', () => {
    expect(service.get('nonexistent')).toBeNull();
  });

  it('should set and get a value', () => {
    service.set('foo', 'bar');
    expect(service.get('foo')).toBe('bar');
  });

  it('should overwrite existing value', () => {
    service.set('foo', 'bar');
    service.set('foo', 'baz');
    expect(service.get('foo')).toBe('baz');
  });

  it('should get and set Telegram config', () => {
    const config = {
      botToken: 'tok',
      chatId: '123',
      dashboardUrl: 'http://localhost',
      enabled: true,
      events: ['error'],
    };
    service.setTelegramConfig(config);
    expect(service.getTelegramConfig()).toEqual(config);
  });

  it('should return null for missing Telegram config', () => {
    expect(service.getTelegramConfig()).toBeNull();
  });
});
