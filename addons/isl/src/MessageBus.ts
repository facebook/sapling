/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Disposable, MessageBusStatus} from './types';
import type {VSCodeAPI} from './vscodeSingleton';

import {LocalWebSocketEventBus} from './LocalWebSocketEventBus';
import vscode from './vscodeSingleton';

export type {MessageBusStatus};

/*
 * Abstraction for the bidirectional communication channel between the
 * Smartlog UI and the "business logic" that talks to EdenSCM, Watchman, etc.
 */
export interface MessageBus {
  onMessage(handler: (event: MessageEvent) => void | Promise<void>): Disposable;
  onChangeStatus(handler: (newStatus: MessageBusStatus) => void | Promise<void>): Disposable;
  // post message accepts string or ArrayBuffer (to send binary data)
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  postMessage(message: any): void;

  /** Force disconnect (for debugging), for supported implementations. */
  forceDisconnect?(durationMs?: number): void;
}

class VSCodeMessageBus {
  constructor(private vscode: VSCodeAPI) {}

  onMessage(handler: (event: MessageEvent<string>) => void | Promise<void>): Disposable {
    window.addEventListener('message', handler);
    const dispose = () => window.removeEventListener('message', handler);
    return {dispose};
  }

  onChangeStatus(handler: (newStatus: MessageBusStatus) => unknown): Disposable {
    // VS Code connections don't close or change status (the webview would just be destroyed if closed)
    handler({type: 'open'});
    // eslint-disable-next-line @typescript-eslint/no-empty-function
    return {dispose: () => {}};
  }

  postMessage(message: string | ArrayBuffer) {
    this.vscode.postMessage(message);
  }
}

const messageBus: MessageBus =
  vscode != null
    ? new VSCodeMessageBus(vscode)
    : new LocalWebSocketEventBus(
        process.env.NODE_ENV === 'development'
          ? // in dev mode, Vite hosts our files for hot-reloading.
            // This means we can't host the ws server on the same port as the page.
            'localhost:3001'
          : // in production, we serve both the static files and ws from the same port
            location.host,
        WebSocket,
      );

declare global {
  interface NodeModule {
    hot?: {
      decline(): void;
    };
  }
}

// We can't allow this file to hot reload, since it creates global state.
// If we did, we'd accumulate global `messageBus`es, which is buggy.
if (import.meta.hot) {
  import.meta.hot?.invalidate();
}

export default messageBus;
