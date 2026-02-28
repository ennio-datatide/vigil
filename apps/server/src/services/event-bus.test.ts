import { describe, expect, it, vi } from 'vitest';
import { EventBus } from './event-bus.js';

describe('EventBus', () => {
  it('should emit and receive session_update events', () => {
    const bus = new EventBus();
    const handler = vi.fn();
    bus.on('session_update', handler);

    bus.emit('session_update', { sessionId: 'abc', status: 'running' });

    expect(handler).toHaveBeenCalledWith({ sessionId: 'abc', status: 'running' });
  });

  it('should emit and receive hook_event events', () => {
    const bus = new EventBus();
    const handler = vi.fn();
    bus.on('hook_event', handler);

    bus.emit('hook_event', {
      sessionId: 'abc',
      eventType: 'PostToolUse',
      toolName: 'Bash',
      payload: {},
      timestamp: Date.now(),
    });

    expect(handler).toHaveBeenCalledTimes(1);
  });

  it('should support removeListener', () => {
    const bus = new EventBus();
    const handler = vi.fn();
    bus.on('session_update', handler);
    bus.off('session_update', handler);

    bus.emit('session_update', { sessionId: 'abc', status: 'running' });

    expect(handler).not.toHaveBeenCalled();
  });
});
