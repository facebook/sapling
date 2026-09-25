/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Disposable, MessageBusStatus, PlatformName} from './types';

import {CLOSED_AND_SHOULD_NOT_RECONNECT_CODE} from 'isl-server/src/constants';
import {logger} from './logger';
import {deserializeFromString, serializeToString} from './serialize';

const CONNECTION_HEARTBEAT_ID = 'isl-connection';

export class LocalWebSocketEventBus {
  static MAX_RECONNECT_CHECK_TIME_MS = 60000;
  static DEFAULT_RECONNECT_CHECK_TIME_MS = 100;
  static CONNECTION_TIMEOUT_MS = 60_000;

  private websocket: WebSocket;
  private status: MessageBusStatus = {type: 'initializing'};
  private exponentialReconnectDelay = LocalWebSocketEventBus.DEFAULT_RECONNECT_CHECK_TIME_MS;
  private queuedMessages: Array<string | ArrayBuffer> = [];
  private reconnectTimer: ReturnType<typeof setTimeout> | undefined;
  private connectionTimer: ReturnType<typeof setTimeout> | undefined;

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
    private params: {
      token?: string;
      cwd?: string;
      extraCwds?: string[];
      sessionId?: string;
      platformName: PlatformName;
    },
  ) {
    // startConnection already assigns to websocket, but we do it here so typescript knows websocket is always defined
    this.websocket = this.startConnection();
  }

  public dispose() {
    if (this.disposed) {
      return;
    }
    this.disposed = true;
    clearTimeout(this.reconnectTimer);
    clearTimeout(this.connectionTimer);
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
    for (const extraCwd of this.params.extraCwds ?? []) {
      wsUrl.searchParams.append('extraCwd', extraCwd);
    }
    const platformName = this.params.platformName;
    if (platformName) {
      wsUrl.searchParams.append('platform', platformName);
    }
    const socket = new this.WebSocketType(wsUrl.href);
    this.websocket = socket;
    this.opening = true;
    this.connectionTimer = setTimeout(() => {
      socket.close();
    }, LocalWebSocketEventBus.CONNECTION_TIMEOUT_MS);
    socket.addEventListener('open', () => {
      if (this.disposed || socket !== this.websocket) {
        return;
      }
      logger.info('websocket open');
      this.opening = true;
      // A proxy can accept the websocket before reaching ISL. Setup callbacks
      // wait for 'open', so probe the application before running them.
      try {
        socket.send(serializeToString({type: 'heartbeat', id: CONNECTION_HEARTBEAT_ID}));
      } catch {
        logger.warn('websocket failed to send readiness heartbeat');
        socket.close();
      }
    });
    socket.addEventListener('message', event => {
      if (this.disposed || socket !== this.websocket) {
        return;
      }
      if (this.opening) {
        let message;
        try {
          message = deserializeFromString(event.data);
        } catch {
          return;
        }
        if (
          message == null ||
          typeof message !== 'object' ||
          !('type' in message) ||
          typeof message.type !== 'string'
        ) {
          return;
        }
        // if any messages were sent while reconnecting, they were queued up.
        // Send them all now that we've reconnected
        try {
          while (this.queuedMessages.length > 0) {
            const queuedMessage = this.queuedMessages[0];
            socket.send(queuedMessage);
            // only dequeue after successfully sending the message
            this.queuedMessages.shift();
          }
        } catch {
          logger.warn('websocket failed to send queued messages');
          socket.close();
          return;
        }

        clearTimeout(this.connectionTimer);
        this.opening = false;
        this.exponentialReconnectDelay = LocalWebSocketEventBus.DEFAULT_RECONNECT_CHECK_TIME_MS;
        this.setStatus({type: 'open'});
        if (
          message.type === 'heartbeat' &&
          'id' in message &&
          message.id === CONNECTION_HEARTBEAT_ID
        ) {
          return;
        }
      }
      for (const handler of this.handlers) {
        handler(event);
      }
    });

    socket.addEventListener('close', event => {
      if (this.disposed || socket !== this.websocket) {
        return;
      }
      clearTimeout(this.connectionTimer);
      this.opening = false;
      logger.info('websocket closed', event.code, event.reason);
      if (event.code === CLOSED_AND_SHOULD_NOT_RECONNECT_CODE) {
        // Don't schedule reconnect if the server told us this is a permanent failure,
        // e.g. invalid token
        this.setStatus({type: 'error', error: event.reason});
        return;
      }
      this.scheduleReconnect();
    });

    return this.websocket;
  }

  private setStatus(status: MessageBusStatus) {
    if (this.status.type === status.type && status.type !== 'error') {
      return;
    }
    this.status = status;
    this.statusChangeHandlers.forEach(handler => handler(status));
  }

  private scheduleReconnect() {
    this.setStatus({type: 'reconnecting'});
    logger.info(`websocket connection closed. Retrying in ${this.exponentialReconnectDelay}ms`);
    clearTimeout(this.reconnectTimer);
    this.reconnectTimer = setTimeout(() => {
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
    this.exponentialReconnectDelay = durationMs;
    this.websocket.close();
  }
}
