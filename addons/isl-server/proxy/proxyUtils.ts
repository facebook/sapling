/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {timingSafeEqual} from 'node:crypto';
import WebSocket from 'ws';

/**
 * Timing safe comparison of tokens coming from strings.
 */
export function areTokensEqual(a: string, b: string): boolean {
  const aBuf = Buffer.from(a);
  const bBuf = Buffer.from(b);
  return aBuf.length === bBuf.length && timingSafeEqual(aBuf, bBuf);
}

/**
 * Proxies between a browser and this server (nest-dev-proxy in front of
 * Agent Home, corp edge proxies) drop a websocket that carries no bytes for
 * about a minute, and ISL is silent while nothing changes in the repo. Half
 * that limit keeps the socket busy with margin to spare.
 */
export const WEBSOCKET_KEEPALIVE_INTERVAL_MS = 30_000;

/**
 * Sends a ping frame to the client every `intervalMs` while the socket is
 * open. Browsers answer pings on their own, so the client needs no change.
 * Returns a function that stops the pings; call it when the socket closes.
 *
 * A failed ping is reported to `onError` and otherwise ignored: the socket's
 * own error/close events decide the connection's fate, and a keepalive must
 * never take the server down or stop pinging the sockets that are still fine.
 */
export function startWebSocketKeepAlive(
  socket: Pick<WebSocket, 'readyState' | 'ping'>,
  intervalMs: number = WEBSOCKET_KEEPALIVE_INTERVAL_MS,
  onError: (error: Error) => void = () => {},
): () => void {
  const report = (error: unknown) => {
    try {
      onError(error instanceof Error ? error : new Error(String(error)));
    } catch {
      // A throwing reporter must not escalate a keepalive hiccup either.
    }
  };
  const timer = setInterval(() => {
    if (socket.readyState !== WebSocket.OPEN) {
      return;
    }
    try {
      // `ws` reports a write failure through the callback and throws only
      // for a socket that is still connecting; guard both the same way.
      socket.ping(undefined, undefined, error => {
        if (error != null) {
          report(error);
        }
      });
    } catch (error) {
      report(error);
    }
  }, intervalMs);
  // Never the reason the process stays alive: the socket itself holds the
  // event loop for as long as the connection exists.
  timer.unref();
  return () => clearInterval(timer);
}
