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
import platform from './platform';
import {deserializeFromString, serializeToString} from './serialize';
import {defer} from 'shared/utils';

export type IncomingMessage = ServerToClientMessage;
export type OutgoingMessage = ClientToServerMessage | ClientToServerMessageWithPayload;

export const debugLogMessageTraffic = {shoudlLog: false};

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
  constructor() {
    platform?.registerServerListeners?.(this);
  }

  private listenersByType = new Map<
    string,
    Set<(message: IncomingMessage) => void | Promise<void>>
  >();
  private incomingListener = messageBus.onMessage(event => {
    const data = deserializeFromString(event.data as string) as IncomingMessage;
    if (debugLogMessageTraffic.shoudlLog) {
      // eslint-disable-next-line no-console
      console.log('%c ⬅ Incoming ', 'color:white;background-color:tomato', data);
    }
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
    dispose?: () => void,
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
          dispose?.();
          found.delete(handler as (event: IncomingMessage) => void | Promise<void>);
        }
      },
    };
  }

  /**
   * Async generator that yields the given type of events.
   * The generator ends when the connection is dropped, or if the callsite
   * uses `break`, `return`, or `throw` to exit the loop body.
   *
   * The event listener will be set up immediately after calling this function,
   * before the first iteration, and teared down when exiting the loop.
   *
   * Typically used in an async function, like:
   *
   * ```
   * async function foo() {
   *   // Set up the listener before sending the request.
   *   const iter = clientToServerAPI.iterateMessageOfType('ResponseType');
   *   clientToServerAPI.postMessage('RequestType', ...);
   *   // Check responses until getting the one we look for.
   *   for await (const event of iter) {
   *     if (matchesRequest(event)) {
   *        if (isGood(event)) {
   *          return ...
   *        } else {
   *          throw ...
   *        }
   *     }
   *   }
   * }
   * ```
   */
  iterateMessageOfType<T extends IncomingMessage['type']>(
    type: T,
  ): AsyncGenerator<IncomingMessage & {type: T}, undefined> {
    // Setup the listener before the first `next()`.
    type Event = IncomingMessage & {type: T};
    const pendingEvents: Event[] = [];
    const pendingPromises: [(value: Event) => void, (reason: Error) => void][] = [];
    let listening = true;
    const listener = this.onMessageOfType(
      type,
      event => {
        const resolveReject = pendingPromises.shift();
        if (resolveReject) {
          resolveReject[0](event);
        } else {
          pendingEvents.push(event);
        }
      },
      () => {
        for (const [, reject] of pendingPromises) {
          reject(new Error('Connection was dropped'));
        }
        pendingPromises.length = 0;
        listening = false;
      },
    );

    // This is a separate function because we want to set the listener
    // immediately when the callsite calls `iterateMessageOfType`.
    return (async function* (): AsyncGenerator<Event, undefined> {
      try {
        while (listening) {
          const event = pendingEvents.shift();
          if (event === undefined) {
            yield new Promise<Event>((resolve, reject) => {
              pendingPromises.push([resolve, reject]);
            });
          } else {
            yield event;
          }
        }
      } catch {
        // ex. connection dropped.
      } finally {
        listener.dispose();
      }
      return undefined;
    })();
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
    if (debugLogMessageTraffic.shoudlLog) {
      // eslint-disable-next-line no-console
      console.log('%c Outgoing ⮕ ', 'color:white;background-color:royalblue', message);
    }
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

  /**
   * Call a callback when a connection is established, or reestablished after a disconnection.
   */
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

  private cwdChangeHandlers: Array<() => unknown> = [];
  onCwdChanged(cb: () => unknown) {
    this.cwdChangeHandlers.push(cb);
    return () => {
      this.cwdChangeHandlers.splice(this.cwdChangeHandlers.indexOf(cb), 1);
    };
  }
  cwdChanged() {
    this.cwdChangeHandlers.forEach(handler => handler());
  }

  /**
   * Call a callback when a connection is established, or reestablished after a disconnection,
   * or the current working directory (and therefore usually repository) changes.
   */
  onSetup(cb: () => (() => unknown) | unknown): () => void {
    const disposeConnectionSubscription = this.onConnectOrReconnect(cb);
    const disposeCwdChange = this.onCwdChanged(cb);

    return () => {
      disposeConnectionSubscription();
      disposeCwdChange();
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
