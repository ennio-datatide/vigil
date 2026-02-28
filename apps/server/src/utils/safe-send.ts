interface SocketLike {
  readyState: number;
  send(data: string): void;
}

export function safeSend(socket: SocketLike, data: string): void {
  if (socket.readyState === 1) {
    try {
      socket.send(data);
    } catch {
      // Socket closed between readyState check and send
    }
  }
}
