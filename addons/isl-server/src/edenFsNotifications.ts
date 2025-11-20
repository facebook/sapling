/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Logger} from './logger';

import type {EventEmitter} from 'node:events';

// Import from the generated files
import {
  EdenFSNotificationsClient,
  type EdenFSSubscription,
} from './__generated__/node-edenfs-notifications-client/index.js';

export type EdenFSSubscriptionOptions = {
  throttle?: number;
  includedRoots?: Array<string>;
  excludedRoots?: Array<string>;
  includedSuffixes?: Array<string>;
  excludedSuffixes?: Array<string>;
  states?: Array<string>;
};

export type EdenFSSubscriptionManager = {
  root: string;
  path: string;
  name: string;
  subscriptionCount: number;
  options: EdenFSSubscriptionOptions;
  emitter: EventEmitter;
  internalSubscription: typeof EdenFSSubscription | null;
};

/**
 * Provides directory watching functionality using EdenFS notify API.
 */
export class EdenFSNotifications {
  private client: EdenFSNotificationsClient;
  private subscriptions: Map<string, EdenFSSubscription> = new Map();

  constructor(
    private logger: Logger,
    private mountPoint?: string,
  ) {
    this.client = new EdenFSNotificationsClient({
      mountPoint: this.mountPoint,
      timeout: 30000,
      edenBinaryPath: process.env.EDEN_PATH ? process.env.EDEN_PATH : 'eden',
    });
  }

  /**
   * Watch a directory recursively for changes using EdenFS notifications.
   */
  public async watchDirectoryRecursive(
    _localDirectoryPath: string,
    _subscriptionName: string,
    _subscriptionOptions?: EdenFSSubscriptionOptions,
  ) {
    // TODO return :Promise<EdenFSSubscription>
  }

  /**
   * Remove a subscription (unwatch)
   */
  public async unwatch(_watchPath: string, _name: string): Promise<void> {}
}
