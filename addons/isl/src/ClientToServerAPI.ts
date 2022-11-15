/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ServerToClientMessage, ClientToServerMessage, Disposable} from './types';

import messageBus from './MessageBus';
import {deserializeFromString, serializeToString} from './serialize';

export type IncomingMessage = ServerToClientMessage;
export type OutgoingMessage = ClientToServerMessage;

export interface ClientToServerAPI {
  dispose(): void;
  onMessageOfType<T extends IncomingMessage['type']>(
    type: T,
    handler: (event: IncomingMessage & {type: T}) => void | Promise<void>,
  ): Disposable;

  postMessage(message: OutgoingMessage): void;
  onConnectOrReconnect(callback: () => unknown): () => void;
}

/**
 * Message passing channel built on top of MessageBus.
 * Use to send and listen for well-typed events with the server
 */
class ClientToServerAPIImpl implements ClientToServerAPI {
  private listenersByType = new Map<
    string,
    Set<(message: IncomingMessage) => void | Promise<void>>
  >();
  private incomingListener = messageBus.onMessage(event => {
    const data = deserializeFromString(event.data as string) as IncomingMessage;
    const {type} = data;
    const listeners = this.listenersByType.get(type);
    if (!listeners) {
      return;
    }
    listeners.forEach(handle => handle(data));
  });

  dispose() {
    this.incomingListener.dispose();
  }

  onMessageOfType<T extends IncomingMessage['type']>(
    type: T,
    handler: (event: IncomingMessage & {type: T}) => void | Promise<void>,
  ): Disposable {
    let found = this.listenersByType.get(type);
    if (found == null) {
      found = new Set();
      this.listenersByType.set(type, found);
    }
    found?.add(handler as (event: IncomingMessage) => void | Promise<void>);
    return {
      dispose: () => {
        const found = this.listenersByType.get(type);
        if (found) {
          found.delete(handler as (event: IncomingMessage) => void | Promise<void>);
        }
      },
    };
  }

  postMessage(message: OutgoingMessage) {
    messageBus.postMessage(serializeToString(message));
  }

  onConnectOrReconnect(callback: () => unknown): () => void {
    let reconnecting = true;
    const disposable = messageBus.onChangeStatus(newStatus => {
      if (newStatus.type === 'reconnecting') {
        reconnecting = true;
      } else if (newStatus.type === 'open') {
        if (reconnecting) {
          callback();
        }
        reconnecting = false;
      }
    });
    return () => disposable.dispose();
  }
}

const clientToServerAPI = new ClientToServerAPIImpl();

declare global {
  interface Window {
    clientToServerAPI?: ClientToServerAPI;
  }
}
window.clientToServerAPI = clientToServerAPI;

export default clientToServerAPI;
