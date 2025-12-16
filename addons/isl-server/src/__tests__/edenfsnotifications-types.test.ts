/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/**
 * Type checking test for fb-edenfs-notifications-client
 * This file verifies that TypeScript types are correctly imported and usable
 */

import {
  EdenFSNotificationsClient,
  EdenFSUtils,
  type ChangesSinceResponse,
  type EdenFSClientOptions,
  type SubscriptionOptions,
} from '../__generated__/node-edenfs-notifications-client/index';

describe('EdenFS Notifications Client Types', () => {
  it('should have correct types for EdenFSNotificationsClient', () => {
    const options: EdenFSClientOptions = {
      mountPoint: '/test/path',
      timeout: 5000,
      edenBinaryPath: 'eden',
    };

    const client = new EdenFSNotificationsClient(options);

    // Type checking test - verifying types are correctly imported
    expect(client).toBeDefined();
    expect(typeof client.getPosition).toBe('function');
    expect(typeof client.getChangesSince).toBe('function');
    expect(typeof client.subscribe).toBe('function');
    expect(client.timeout).toBe(5000);
  });

  it('should have correct types for subscription options', () => {
    const subscriptionOptions: SubscriptionOptions = {
      mountPoint: '/test/path',
      throttle: 100,
      includeVcsRoots: true,
      includedSuffixes: ['.ts', '.js'],
      excludedRoots: ['node_modules'],
    };

    expect(subscriptionOptions).toBeDefined();
    expect(subscriptionOptions.throttle).toBe(100);
  });

  it('should have correct types for EdenFSUtils', () => {
    const pathBytes = [104, 101, 108, 108, 111]; // "hello" in bytes
    const path = EdenFSUtils.bytesToPath(pathBytes);

    expect(path).toBe('hello');
    expect(typeof path).toBe('string');
  });

  it('should type check changes response correctly', () => {
    const mockResponse: ChangesSinceResponse = {
      changes: [
        {
          SmallChange: {
            Added: {
              path: [116, 101, 115, 116], // "test"
              file_type: 'Regular',
            },
          },
        },
      ],
      to_position: 'abc123',
    };

    expect(mockResponse.changes).toHaveLength(1);
    expect(mockResponse.to_position).toBe('abc123');
  });
});
