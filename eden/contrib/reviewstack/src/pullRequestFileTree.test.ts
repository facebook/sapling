/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DisplayChange} from './coalesceRenamedFiles';
import type {TreeEntry} from './github/types';

import buildPullRequestFileTree from './pullRequestFileTree';

function entry(name: string): TreeEntry {
  return {name, oid: name, mode: 33188, path: name, type: 'blob'};
}

function added(path: string): DisplayChange {
  const parts = path.split('/');
  const name = parts.pop() ?? '';
  return {type: 'add', basePath: parts.join('/'), entry: entry(name)};
}

test('groups changed files into sorted directory levels', () => {
  expect(buildPullRequestFileTree([
    added('root.ts'),
    added('common/z.ts'),
    added('common/model_zoo/b.ts'),
    added('common/model_zoo/a.ts'),
    added('api/client.ts'),
  ])).toEqual([
    {
      kind: 'directory',
      name: 'api',
      path: 'api',
      children: [expect.objectContaining({kind: 'file', name: 'client.ts', path: 'api/client.ts'})],
    },
    {
      kind: 'directory',
      name: 'common',
      path: 'common',
      children: [
        {
          kind: 'directory',
          name: 'model_zoo',
          path: 'common/model_zoo',
          children: [
            expect.objectContaining({kind: 'file', name: 'a.ts'}),
            expect.objectContaining({kind: 'file', name: 'b.ts'}),
          ],
        },
        expect.objectContaining({kind: 'file', name: 'z.ts'}),
      ],
    },
    expect.objectContaining({kind: 'file', name: 'root.ts', path: 'root.ts'}),
  ]);
});
