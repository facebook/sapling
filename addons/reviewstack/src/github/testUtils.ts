/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {TreeEntry} from './types';

type MockEntryBase = {
  name: string;
  oid: string;
};

type MockBlob = MockEntryBase & {
  type: 'blob';
};

export type MockTree = MockEntryBase & {
  type: 'tree';
  entries: MockTreeEntry[];
};

export type MockTreeEntry = MockBlob | MockTree;

export function createTreeEntryFromMock(mockEntry: MockTreeEntry, path: string): TreeEntry {
  return {
    ...mockEntry,
    path,
    object: null,
    mode: 0o100644,
  };
}

export function oid(seed: string, length = 40): string {
  const count = length / seed.length;
  if (Number.isInteger(count)) {
    return seed.repeat(count);
  } else {
    throw new Error(`length of seed '${seed}' not divisible by ${length}`);
  }
}
