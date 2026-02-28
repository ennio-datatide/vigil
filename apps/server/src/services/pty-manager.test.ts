import { afterEach, describe, expect, it } from 'vitest';
import { PtyManager } from './pty-manager.js';

describe('PtyManager', () => {
  let manager: PtyManager;

  afterEach(() => {
    manager?.disposeAll();
  });

  it('should create a PTY process', () => {
    manager = new PtyManager();

    const pty = manager.create('test-1', '/bin/cat', [], {
      cwd: '/tmp',
    });

    expect(pty).toBeDefined();
    expect(manager.isAlive('test-1')).toBe(true);
  });

  it('should report non-existent sessions as not alive', () => {
    manager = new PtyManager();
    expect(manager.isAlive('nonexistent')).toBe(false);
  });

  it('should write data to a PTY', () => {
    manager = new PtyManager();

    manager.create('test-write', '/bin/cat', [], { cwd: '/tmp' });

    // Should not throw
    expect(() => manager.write('test-write', 'hello\n')).not.toThrow();
  });

  it('should resize a PTY', () => {
    manager = new PtyManager();

    manager.create('test-resize', '/bin/cat', [], {
      cwd: '/tmp',
      cols: 80,
      rows: 24,
    });

    // Should not throw
    expect(() => manager.resize('test-resize', 120, 40)).not.toThrow();
  });

  it('should kill a PTY process', () => {
    manager = new PtyManager();

    manager.create('test-kill', '/bin/cat', [], { cwd: '/tmp' });
    expect(manager.isAlive('test-kill')).toBe(true);

    manager.kill('test-kill');
    expect(manager.isAlive('test-kill')).toBe(false);
  });

  it('should not throw when killing a non-existent session', () => {
    manager = new PtyManager();
    expect(() => manager.kill('nonexistent')).not.toThrow();
  });

  it('should track active sessions', () => {
    manager = new PtyManager();

    manager.create('s1', '/bin/cat', [], { cwd: '/tmp' });
    manager.create('s2', '/bin/cat', [], { cwd: '/tmp' });

    const active = manager.getActiveSessions();
    expect(active).toContain('s1');
    expect(active).toContain('s2');
    expect(active).toHaveLength(2);
  });

  it('should dispose all PTY processes', () => {
    manager = new PtyManager();

    manager.create('d1', '/bin/cat', [], { cwd: '/tmp' });
    manager.create('d2', '/bin/cat', [], { cwd: '/tmp' });

    manager.disposeAll();

    expect(manager.isAlive('d1')).toBe(false);
    expect(manager.isAlive('d2')).toBe(false);
    expect(manager.getActiveSessions()).toHaveLength(0);
  });

  it('should replace existing PTY when creating with same session ID', () => {
    manager = new PtyManager();

    const pty1 = manager.create('dup', '/bin/cat', [], { cwd: '/tmp' });
    const pty2 = manager.create('dup', '/bin/cat', [], { cwd: '/tmp' });

    // Should still only have one entry
    expect(manager.getActiveSessions()).toHaveLength(1);
    expect(pty2).not.toBe(pty1);
  });

  it('should write and resize no-ops for non-existent sessions', () => {
    manager = new PtyManager();

    // These should not throw
    expect(() => manager.write('nope', 'data')).not.toThrow();
    expect(() => manager.resize('nope', 80, 24)).not.toThrow();
  });
});
