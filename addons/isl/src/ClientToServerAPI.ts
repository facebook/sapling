/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {
  ServerToClientMessage,
  ClientToServerMessage,
  Disposable,
  ClientToServerMessageWithPayload,
} from './types';

import messageBus from './MessageBus';
import {deserializeFromString, serializeToString} from './serialize';
import {defer} from 'shared/utils';

export type IncomingMessage = ServerToClientMessage;
export type OutgoingMessage = ClientToServerMessage | ClientToServerMessageWithPayload;

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

  /**
   * Returns the next message in the stream of `type` that also matches the given predicate.
   */
  nextMessageMatching<T extends IncomingMessage['type']>(
    type: T,
    test: (message: IncomingMessage & {type: T}) => boolean,
  ): Promise<IncomingMessage & {type: T}> {
    const deferred = defer<IncomingMessage & {type: T}>();
    let dispose: Disposable | null = this.onMessageOfType(type, message => {
      if (test(message)) {
        dispose?.dispose();
        dispose = null;
        deferred.resolve(message);
      }
    });

    return deferred.promise;
  }

  postMessage(message: ClientToServerMessage) {
    messageBus.postMessage(serializeToString(message));
  }

  /**
   * Post a message with an ArrayBuffer binary payload.
   * No need to specify `hasBinaryPayload: true` in your message.
   * This actually sends two messages: the JSON text message, then the binary payload, and reconnects them on the server.
   */
  postMessageWithPayload(
    // Omit lets callers not include hasBinaryPayload themselves, since it's implicit in calling postMessageWithPayload.
    message: Omit<ClientToServerMessageWithPayload, 'hasBinaryPayload'>,
    payload: ArrayBuffer,
  ) {
    messageBus.postMessage(
      serializeToString({...message, hasBinaryPayload: true} as ClientToServerMessageWithPayload),
    );
    messageBus.postMessage(payload);
  }

  onConnectOrReconnect(callback: () => (() => unknown) | unknown): () => void {
    let reconnecting = true;
    let disposeCallback: (() => unknown) | unknown = undefined;
    const disposable = messageBus.onChangeStatus(newStatus => {
      if (newStatus.type === 'reconnecting') {
        reconnecting = true;
      } else if (newStatus.type === 'open') {
        if (reconnecting) {
          disposeCallback = callback();
        }
        reconnecting = false;
      }
    });
    return () => {
      disposable.dispose();
      typeof disposeCallback === 'function' && disposeCallback?.();
    };
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
