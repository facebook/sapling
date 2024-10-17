/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Disposable, MessageBusStatus} from './types';

export type {MessageBusStatus};

/*
 * Abstraction for the bidirectional communication channel between the
 * ISL UI and the "business logic" that talks to Sapling, Watchman, etc.
 *
 * Every platform (BrowserPlatform VSCodeWebviewPlatform, etc) will have a single MessageBus instance.
 */
export interface MessageBus {
  onMessage(handler: (event: MessageEvent) => void | Promise<void>): Disposable;
  onChangeStatus(handler: (newStatus: MessageBusStatus) => void | Promise<void>): Disposable;
  postMessage(message: string): void;

  /** Force disconnect (for debugging), for supported implementations. */
  forceDisconnect?(durationMs?: number): void;
}
