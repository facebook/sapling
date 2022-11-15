/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {groupBy, splitPath} from './utils';

describe('groupBy', () => {
  test('returns Map of values keyed by result of given function', () => {
    const values = [
      {type: 'foo', value: 0},
      {type: 'bar', value: 5},
      {type: 'baz', value: 3},
      {type: 'foo', value: 1},
    ];

    const expected = new Map([
      [
        'foo',
        [
          {type: 'foo', value: 0},
          {type: 'foo', value: 1},
        ],
      ],
      ['bar', [{type: 'bar', value: 5}]],
      ['baz', [{type: 'baz', value: 3}]],
    ]);

    expect(groupBy(values, value => value.type)).toEqual(expected);
  });

  test('excludes null keys', () => {
    const values = [
      {type: 'foo', value: 0},
      {type: null, value: 5},
    ];

    const expected = new Map([['foo', [{type: 'foo', value: 0}]]]);

    expect(groupBy(values, value => value.type)).toEqual(expected);
  });
});

describe('splitPath', () => {
  test('splits path into dirname and basename', () => {
    expect(splitPath('')).toEqual(['', '']);
    expect(splitPath('foo')).toEqual(['', 'foo']);
    expect(splitPath('/foo')).toEqual(['', 'foo']);
    expect(splitPath('/foo/bar')).toEqual(['/foo', 'bar']);
    expect(splitPath('foo/bar')).toEqual(['foo', 'bar']);
    expect(splitPath('foo/bar/baz')).toEqual(['foo/bar', 'baz']);
  });
});
