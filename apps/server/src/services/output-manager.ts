const MAX_LINES = 1000;

type Subscriber = (data: string) => void;

interface SessionBuffer {
  lines: string[];
  subscribers: Set<Subscriber>;
}

export class OutputManager {
  private buffers = new Map<string, SessionBuffer>();

  createBuffer(sessionId: string): void {
    if (!this.buffers.has(sessionId)) {
      this.buffers.set(sessionId, { lines: [], subscribers: new Set() });
    }
  }

  append(sessionId: string, data: string): void {
    const buf = this.buffers.get(sessionId);
    if (!buf) return;

    buf.lines.push(data);
    if (buf.lines.length > MAX_LINES) {
      buf.lines.splice(0, buf.lines.length - MAX_LINES);
    }

    for (const cb of buf.subscribers) {
      cb(data);
    }
  }

  subscribe(sessionId: string, cb: Subscriber): () => void {
    const buf = this.buffers.get(sessionId);
    if (!buf) return () => {};

    buf.subscribers.add(cb);
    return () => {
      buf.subscribers.delete(cb);
    };
  }

  getHistory(sessionId: string): string {
    const buf = this.buffers.get(sessionId);
    if (!buf) return '';
    return buf.lines.join('');
  }

  hasSession(sessionId: string): boolean {
    return this.buffers.has(sessionId);
  }

  getActiveSessions(): string[] {
    return Array.from(this.buffers.keys());
  }

  remove(sessionId: string): void {
    this.buffers.delete(sessionId);
  }

  disposeAll(): void {
    this.buffers.clear();
  }
}
