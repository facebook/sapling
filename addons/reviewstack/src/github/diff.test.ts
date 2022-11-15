/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Diff} from './diffTypes';
import type {MockTree} from './testUtils';

import {getPathForChange} from '../utils';
import TestGitHubClient from './TestGitHubClient';
import {diffTree} from './diff';
import {diffVersions} from './diffVersions';
import {oid} from './testUtils';

describe('diffTree', () => {
  type Change = {
    type: 'add' | 'remove' | 'modify';
    path: string;
  };

  type TestCase = {
    mockBaseTree: MockTree;
    mockHeadTree: MockTree;
    expected: Change[];
  };

  async function testDiffTree({mockBaseTree, mockHeadTree, expected}: TestCase) {
    const diff: Diff = [];
    const client = new TestGitHubClient([mockBaseTree, mockHeadTree]);
    const baseTree = await client.getTree(mockBaseTree.oid);
    const headTree = await client.getTree(mockHeadTree.oid);

    if (baseTree != null && headTree != null) {
      await diffTree(diff, '', baseTree, headTree, client);
    }

    const actual: Change[] = diff.map(change => ({
      type: change.type,
      path: getPathForChange(change),
    }));

    expect(actual).toEqual(expected);
    expect(() => {
      diffVersions(diff, diff);
    }).not.toThrowError();
  }

  test('same tree', async () => {
    const mockTree: MockTree = {
      type: 'tree',
      name: '',
      oid: 'base',
      entries: [
        {
          type: 'tree',
          name: 'foo',
          oid: oid('a'),
          entries: [
            {
              type: 'tree',
              name: 'bar',
              oid: oid('b'),
              entries: [
                {
                  type: 'blob',
                  name: 'qux',
                  oid: oid('c'),
                },
              ],
            },
            {
              type: 'blob',
              name: 'baz',
              oid: oid('d'),
            },
          ],
        },
      ],
    };

    await testDiffTree({mockBaseTree: mockTree, mockHeadTree: mockTree, expected: []});
  });

  test('base blob was removed in head tree', async () => {
    const mockBaseTree: MockTree = {
      type: 'tree',
      name: '',
      oid: 'base',
      entries: [
        {
          type: 'blob',
          name: 'foo',
          oid: oid('a'),
        },
      ],
    };
    const mockHeadTree: MockTree = {
      type: 'tree',
      name: '',
      oid: 'head',
      entries: [],
    };

    await testDiffTree({mockBaseTree, mockHeadTree, expected: [{type: 'remove', path: 'foo'}]});
  });

  test('base tree was removed in head tree', async () => {
    const mockBaseTree: MockTree = {
      type: 'tree',
      name: '',
      oid: 'base',
      entries: [
        {
          type: 'tree',
          name: 'foo',
          oid: oid('a'),
          entries: [
            {
              type: 'blob',
              name: 'bar',
              oid: oid('b'),
            },
          ],
        },
      ],
    };
    const mockHeadTree: MockTree = {
      type: 'tree',
      name: '',
      oid: 'head',
      entries: [],
    };

    await testDiffTree({mockBaseTree, mockHeadTree, expected: [{type: 'remove', path: 'foo/bar'}]});
  });

  test('head blob was introduced in head tree', async () => {
    const mockBaseTree: MockTree = {
      type: 'tree',
      name: '',
      oid: 'base',
      entries: [],
    };
    const mockHeadTree: MockTree = {
      type: 'tree',
      name: '',
      oid: 'head',
      entries: [
        {
          type: 'blob',
          name: 'foo',
          oid: oid('a'),
        },
      ],
    };

    await testDiffTree({mockBaseTree, mockHeadTree, expected: [{type: 'add', path: 'foo'}]});
  });

  test('head tree was introduced in head tree', async () => {
    const mockBaseTree: MockTree = {
      type: 'tree',
      name: '',
      oid: 'base',
      entries: [],
    };
    const mockHeadTree: MockTree = {
      type: 'tree',
      name: '',
      oid: 'head',
      entries: [
        {
          type: 'tree',
          name: 'foo',
          oid: oid('a'),
          entries: [
            {
              type: 'blob',
              name: 'bar',
              oid: oid('b'),
            },
          ],
        },
      ],
    };

    await testDiffTree({mockBaseTree, mockHeadTree, expected: [{type: 'add', path: 'foo/bar'}]});
  });

  test('blob was changed', async () => {
    const mockBaseTree: MockTree = {
      type: 'tree',
      name: '',
      oid: 'base',
      entries: [
        {
          type: 'blob',
          name: 'foo',
          oid: oid('a'),
        },
      ],
    };
    const mockHeadTree: MockTree = {
      type: 'tree',
      name: '',
      oid: 'head',
      entries: [
        {
          type: 'blob',
          name: 'foo',
          oid: oid('b'),
        },
      ],
    };

    await testDiffTree({mockBaseTree, mockHeadTree, expected: [{type: 'modify', path: 'foo'}]});
  });

  test('blob was replaced with a tree', async () => {
    const mockBaseTree: MockTree = {
      type: 'tree',
      name: '',
      oid: 'base',
      entries: [
        {
          type: 'blob',
          name: 'foo',
          oid: oid('a'),
        },
      ],
    };
    const mockHeadTree: MockTree = {
      type: 'tree',
      name: '',
      oid: 'head',
      entries: [
        {
          type: 'tree',
          name: 'foo',
          oid: oid('b'),
          entries: [
            {
              type: 'blob',
              name: 'bar',
              oid: oid('c'),
            },
          ],
        },
      ],
    };

    await testDiffTree({
      mockBaseTree,
      mockHeadTree,
      expected: [
        {type: 'remove', path: 'foo'},
        {type: 'add', path: 'foo/bar'},
      ],
    });
  });

  test('tree was replaced with a blob', async () => {
    const mockBaseTree: MockTree = {
      type: 'tree',
      name: '',
      oid: 'base',
      entries: [
        {
          type: 'tree',
          name: 'foo',
          oid: oid('a'),
          entries: [
            {
              type: 'blob',
              name: 'bar',
              oid: oid('b'),
            },
          ],
        },
      ],
    };
    const mockHeadTree: MockTree = {
      type: 'tree',
      name: '',
      oid: 'head',
      entries: [
        {
          type: 'blob',
          name: 'foo',
          oid: oid('c'),
        },
      ],
    };

    await testDiffTree({
      mockBaseTree,
      mockHeadTree,
      expected: [
        {type: 'add', path: 'foo'},
        {type: 'remove', path: 'foo/bar'},
      ],
    });
  });
});
