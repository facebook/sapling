/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {PathTree} from '../pathTree';

import {buildPathTree} from '../pathTree';

type FakeData = {name: string};
describe('pathTree', () => {
  it('constructs tree', () => {
    const tree = buildPathTree<FakeData>({
      'a/b/file1.txt': {name: 'file1.txt'},
      'a/b/file2.txt': {name: 'file2.txt'},
      'a/file3.txt': {name: 'file3.txt'},
      'a/d/e/f/file4.txt': {name: 'file4.txt'},
      'q/file5.txt': {name: 'file5.txt'},
      'file6.txt': {name: 'file5.txt'},
    });

    expect(tree).toEqual(
      testTree([
        [
          'a',
          [
            [
              'b',
              [
                ['file1.txt', {name: 'file1.txt'}],
                ['file2.txt', {name: 'file2.txt'}],
              ],
            ],
            ['file3.txt', {name: 'file3.txt'}],
            ['d/e/f', [['file4.txt', {name: 'file4.txt'}]]],
          ],
        ],
        ['q', [['file5.txt', {name: 'file5.txt'}]]],
        ['file6.txt', {name: 'file5.txt'}],
      ]),
    );
  });

  it('groups out of order elements tree', () => {
    const tree = buildPathTree<FakeData>({
      'a/b/file1.txt': {name: 'file1.txt'},
      'file6.txt': {name: 'file5.txt'},
      'a/file3.txt': {name: 'file3.txt'},
      'a/d/e/f/file4.txt': {name: 'file4.txt'},
      'q/file5.txt': {name: 'file5.txt'},
      'a/b/file2.txt': {name: 'file2.txt'},
    });

    expect(tree).toEqual(
      testTree([
        [
          'a',
          [
            [
              'b',
              [
                ['file1.txt', {name: 'file1.txt'}],
                ['file2.txt', {name: 'file2.txt'}],
              ],
            ],
            ['file3.txt', {name: 'file3.txt'}],
            ['d/e/f', [['file4.txt', {name: 'file4.txt'}]]],
          ],
        ],
        ['q', [['file5.txt', {name: 'file5.txt'}]]],
        ['file6.txt', {name: 'file5.txt'}],
      ]),
    );
  });

  it('groups with condensed prefixes', () => {
    const tree = buildPathTree<FakeData>({
      'a/b/file1.txt': {name: 'file1.txt'},
      'a/b/file2.txt': {name: 'file2.txt'},
      'a/b/c/d/e/file3.txt': {name: 'file3.txt'},
      'a/b/c/d/e/file4.txt': {name: 'file4.txt'},
    });

    expect(tree).toEqual(
      testTree([
        [
          'a/b',
          [
            ['file1.txt', {name: 'file1.txt'}],
            ['file2.txt', {name: 'file2.txt'}],
            [
              'c/d/e',
              [
                ['file3.txt', {name: 'file3.txt'}],
                ['file4.txt', {name: 'file4.txt'}],
              ],
            ],
          ],
        ],
      ]),
    );
  });

  it('testtree util works', () => {
    expect(
      testTree([
        [
          'a',
          [
            [
              'b',
              [
                ['file1.txt', {name: 'file1.txt'}],
                ['file2.txt', {name: 'file2.txt'}],
              ],
            ],
            ['file3.txt', {name: 'file3.txt'}],
          ],
        ],
        ['file4.txt', {name: 'file4.txt'}],
      ]),
    ).toEqual(
      new Map<string, FakeData | Map<string, FakeData | Map<string, FakeData>>>([
        [
          'a',
          new Map<string, FakeData | Map<string, FakeData>>([
            [
              'b',
              new Map([
                ['file1.txt', {name: 'file1.txt'}],
                ['file2.txt', {name: 'file2.txt'}],
              ]),
            ],
            ['file3.txt', {name: 'file3.txt'}],
          ]),
        ],
        ['file4.txt', {name: 'file4.txt'}],
      ]),
    );
  });
});

type Data = Array<[string, Data | FakeData]>;
// make testing slightly easier so we don't need to constuct maps in expected result
function testTree(data: Data): PathTree<FakeData> {
  return new Map(
    data.map(([k, v]): [string, FakeData | PathTree<FakeData>] =>
      Array.isArray(v) ? [k, testTree(v)] : [k, v],
    ),
  );
}
