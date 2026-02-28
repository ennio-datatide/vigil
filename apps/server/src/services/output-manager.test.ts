import { describe, it, expect, vi } from 'vitest';
import { OutputManager } from './output-manager.js';

describe('OutputManager', () => {
  it('should create and track a buffer', () => {
    const mgr = new OutputManager();
    mgr.createBuffer('sess-1');
    expect(mgr.hasSession('sess-1')).toBe(true);
    expect(mgr.hasSession('sess-2')).toBe(false);
  });

  it('should append data and retrieve history', () => {
    const mgr = new OutputManager();
    mgr.createBuffer('sess-1');
    mgr.append('sess-1', 'hello ');
    mgr.append('sess-1', 'world');
    expect(mgr.getHistory('sess-1')).toBe('hello world');
  });

  it('should notify subscribers on append', () => {
    const mgr = new OutputManager();
    mgr.createBuffer('sess-1');
    const cb = vi.fn();
    mgr.subscribe('sess-1', cb);
    mgr.append('sess-1', 'data');
    expect(cb).toHaveBeenCalledWith('data');
  });

  it('should unsubscribe correctly', () => {
    const mgr = new OutputManager();
    mgr.createBuffer('sess-1');
    const cb = vi.fn();
    const unsub = mgr.subscribe('sess-1', cb);
    unsub();
    mgr.append('sess-1', 'data');
    expect(cb).not.toHaveBeenCalled();
  });

  it('should trim buffer to 1000 lines', () => {
    const mgr = new OutputManager();
    mgr.createBuffer('sess-1');
    for (let i = 0; i < 1050; i++) {
      mgr.append('sess-1', `line ${i}\n`);
    }
    const history = mgr.getHistory('sess-1');
    const lines = history.split('\n').filter(Boolean);
    expect(lines.length).toBe(1000);
    expect(lines[0]).toBe('line 50');
  });

  it('should return empty string for unknown session history', () => {
    const mgr = new OutputManager();
    expect(mgr.getHistory('unknown')).toBe('');
  });

  it('should return noop unsubscribe for unknown session', () => {
    const mgr = new OutputManager();
    const unsub = mgr.subscribe('unknown', () => {});
    expect(() => unsub()).not.toThrow();
  });

  it('should ignore append for unknown session', () => {
    const mgr = new OutputManager();
    expect(() => mgr.append('unknown', 'data')).not.toThrow();
  });

  it('should list active sessions', () => {
    const mgr = new OutputManager();
    mgr.createBuffer('sess-1');
    mgr.createBuffer('sess-2');
    expect(mgr.getActiveSessions()).toEqual(['sess-1', 'sess-2']);
  });

  it('should remove a session buffer', () => {
    const mgr = new OutputManager();
    mgr.createBuffer('sess-1');
    mgr.remove('sess-1');
    expect(mgr.hasSession('sess-1')).toBe(false);
  });

  it('should dispose all buffers', () => {
    const mgr = new OutputManager();
    mgr.createBuffer('sess-1');
    mgr.createBuffer('sess-2');
    mgr.disposeAll();
    expect(mgr.getActiveSessions()).toEqual([]);
  });

  it('should not create duplicate buffer', () => {
    const mgr = new OutputManager();
    mgr.createBuffer('sess-1');
    mgr.append('sess-1', 'data');
    mgr.createBuffer('sess-1');
    expect(mgr.getHistory('sess-1')).toBe('data');
  });
});
