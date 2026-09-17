/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {AddChange, Diff, RemoveChange} from './github/diffTypes';
import type {CommitComparisonFile} from './github/restApiTypes';
import type {TreeEntry} from './github/types';

import coalesceRenamedFiles from './coalesceRenamedFiles';

function entry(name: string, oid: string, mode = 33188): TreeEntry {
  return {name, oid, mode, path: name, type: 'blob'};
}

function remove(name: string, oid: string, mode?: number): RemoveChange {
  return {type: 'remove', basePath: '', entry: entry(name, oid, mode)};
}

function add(name: string, oid: string, mode?: number): AddChange {
  return {type: 'add', basePath: '', entry: entry(name, oid, mode)};
}

function renamedFile(filename: string, previousFilename: string): CommitComparisonFile {
  return {
    additions: 1,
    deletions: 1,
    filename,
    previousFilename,
    status: 'renamed',
  };
}

test('pairs an unchanged removal and addition as one renamed file', () => {
  const changes = coalesceRenamedFiles([remove('old.ts', 'blob'), add('new.ts', 'blob')]);

  expect(changes).toEqual([
    {type: 'rename', before: remove('old.ts', 'blob'), after: add('new.ts', 'blob')},
  ]);
});

test('does not hide content or mode changes', () => {
  const diff: Diff = [
    remove('old-content.ts', 'before'),
    add('new-content.ts', 'after'),
    remove('old-mode.ts', 'same', 33188),
    add('new-mode.ts', 'same', 33261),
  ];

  expect(coalesceRenamedFiles(diff)).toEqual(diff);
});

test('pairs an edited rename using GitHub comparison metadata', () => {
  const removal = remove('old.ts', 'before');
  const addition = add('new.ts', 'after');

  expect(coalesceRenamedFiles([removal, addition], [renamedFile('new.ts', 'old.ts')])).toEqual([
    {type: 'rename', before: removal, after: addition},
  ]);
});

test('ignores rename metadata when either path is absent from the diff', () => {
  const diff: Diff = [remove('old.ts', 'before'), add('different.ts', 'after')];

  expect(coalesceRenamedFiles(diff, [renamedFile('new.ts', 'old.ts')])).toEqual(diff);
});

test('pairs duplicate blobs one-to-one and leaves copies visible', () => {
  const firstRemoval = remove('old-a.ts', 'same');
  const secondRemoval = remove('old-b.ts', 'same');
  const addition = add('new-a.ts', 'same');

  expect(coalesceRenamedFiles([firstRemoval, secondRemoval, addition])).toEqual([
    {type: 'rename', before: firstRemoval, after: addition},
    secondRemoval,
  ]);
});
