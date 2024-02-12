/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/*
 * The SharedWorkers running the code for diffServiceWorker.ts broadcast their
 * availability to the host page via a BroadcastChannel. This file codifies the
 * protocol for the channel.
 */

export const AVAILABILITY_METHOD = 'diff-service-availability';

/** Union type with one variant, as only one message type is currently used. */
export type BroadcastMessage = {
  method: typeof AVAILABILITY_METHOD;
  workerName: string;
  available: boolean;
};

/**
 * Use this to create the BroadcastChannel to ensure the name is specified
 * consistently on both ends of the channel.
 */
export function createDiffServiceBroadcastChannel(): BroadcastChannel {
  return new BroadcastChannel('diff-service');
}

/** When creating new SharedWorkers, the index should start at 0. */
export function createWorkerName(index: number) {
  return `diff-service-worker-${index}`;
}

export function parseWorkerIndex(workerName: string): number | null {
  const match = workerName.match(/^diff-service-worker-(\d+)$/);
  return match != null ? parseInt(match[1], 10) : null;
}
