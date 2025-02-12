/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Disposable, MessageBusStatus, PlatformName} from './types';

import {CLOSED_AND_SHOULD_NOT_RECONNECT_CODE} from 'isl-server/src/constants';
import {logger} from './logger';

export class LocalWebSocketEventBus {
  static MAX_RECONNECT_CHECK_TIME_MS = 60000;
  static DEFAULT_RECONNECT_CHECK_TIME_MS = 100;

  private websocket: WebSocket;
  private status: MessageBusStatus = {type: 'initializing'};
  private exponentialReconnectDelay = LocalWebSocketEventBus.DEFAULT_RECONNECT_CHECK_TIME_MS;
  private queuedMessages: Array<string | ArrayBuffer> = [];

  // A sub-state of "status", used by `startConnection` to avoid creating multiple
  // websockets while connecting.
  //
  // status.type: | 'initializing' | 'open' | 'reconnecting' | 'open'
  //     opening: | true           | false  | false | true   | false
  //                                         ^^^^^^^ reconnect setTimeout
  private opening = false;

  private handlers: Array<(event: MessageEvent<string>) => void | Promise<void>> = [];
  private statusChangeHandlers: Array<(newStatus: MessageBusStatus) => unknown> = [];

  private disposed = false;

  /**
   * @param host to use when creating the Web Socket to talk to the server. Should
   * include the hostname and optionally, a port, e.g., "localhost:3001" or "example.com".
   */
  constructor(
    private host: string,
    private WebSocketType: typeof WebSocket,
    private params: {token?: string; cwd?: string; sessionId?: string; platformName: PlatformName},
  ) {
    // startConnection already assigns to websocket, but we do it here so typescript knows websocket is always defined
    this.websocket = this.startConnection();
  }

  public dispose() {
    if (this.disposed) {
      return;
    }
    this.disposed = true;
    this.websocket.close();
  }

  private startConnection(): WebSocket {
    if (this.disposed || this.opening || this.status.type === 'open') {
      return this.websocket;
    }
    const wsProtocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const wsUrl = new URL(`${wsProtocol}//${this.host}/ws`);
    const token = this.params.token;
    if (token) {
      wsUrl.searchParams.append('token', token);
    }
    const cwdParam = this.params.cwd;
    if (cwdParam) {
      const cwd = decodeURIComponent(cwdParam);
      wsUrl.searchParams.append('cwd', cwd);
    }
    const sessionIdParam = this.params.sessionId;
    if (sessionIdParam) {
      const sessionId = decodeURIComponent(sessionIdParam);
      wsUrl.searchParams.append('sessionId', sessionId);
    }
    const platformName = this.params.platformName;
    if (platformName) {
      wsUrl.searchParams.append('platform', platformName);
    }
    this.websocket = new this.WebSocketType(wsUrl.href);
    this.opening = true;
    this.websocket.addEventListener('open', event => {
      logger.info('websocket open', event);
      this.opening = false;
      this.exponentialReconnectDelay = LocalWebSocketEventBus.DEFAULT_RECONNECT_CHECK_TIME_MS;

      this.websocket.addEventListener('message', e => {
        for (const handler of this.handlers) {
          handler(e);
        }
      });

      // if any messages were sent while reconnecting, they were queued up.
      // Send them all now that we've reconnected
      while (this.queuedMessages.length > 0) {
        const queuedMessage = this.queuedMessages[0];
        this.websocket.send(queuedMessage);
        // only dequeue after successfully sending the message
        this.queuedMessages.shift();
      }

      this.setStatus({type: 'open'});
    });

    this.websocket.addEventListener('close', event => {
      this.opening = false;
      if (event.code === CLOSED_AND_SHOULD_NOT_RECONNECT_CODE) {
        // Don't schedule reconnect if the server told us this is a permanent failure,
        // e.g. invalid token
        this.setStatus({type: 'error', error: event.reason});
        return;
      }
      if (!this.disposed) {
        this.scheduleReconnect();
      }
    });

    return this.websocket;
  }

  private setStatus(status: MessageBusStatus) {
    this.status = status;
    this.statusChangeHandlers.forEach(handler => handler(status));
  }

  private scheduleReconnect() {
    this.setStatus({type: 'reconnecting'});
    logger.info(`websocket connection closed. Retrying in ${this.exponentialReconnectDelay}ms`);
    setTimeout(() => {
      this.startConnection();
    }, this.exponentialReconnectDelay);

    this.exponentialReconnectDelay = Math.min(
      this.exponentialReconnectDelay * 2,
      LocalWebSocketEventBus.MAX_RECONNECT_CHECK_TIME_MS,
    );
  }

  onMessage(handler: (event: MessageEvent<string>) => void | Promise<void>): Disposable {
    // we need to track handlers ourself instead of directly calling this.websocket.addEventListener here,
    // since we'll get a new WebSocket on reconnect.
    this.handlers.push(handler);
    const dispose = () => {
      const foundIndex = this.handlers.indexOf(handler);
      if (foundIndex !== -1) {
        this.handlers.splice(foundIndex, 1);
      }
    };
    return {dispose};
  }

  postMessage(message: string) {
    if (this.status.type === 'open') {
      this.websocket.send(message);
    } else {
      this.queuedMessages.push(message);
    }
  }

  onChangeStatus(handler: (newStatus: MessageBusStatus) => void | Promise<void>): Disposable {
    this.statusChangeHandlers.push(handler);
    handler(this.status); // seed with current status
    const dispose = () => {
      const foundIndex = this.statusChangeHandlers.indexOf(handler);
      if (foundIndex !== -1) {
        this.statusChangeHandlers.splice(foundIndex, 1);
      }
    };
    return {dispose};
  }

  forceDisconnect(durationMs = 1000) {
    this.websocket.close();
    this.exponentialReconnectDelay = durationMs;
    this.scheduleReconnect();
  }
}
