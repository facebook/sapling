/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {groupBy} from './utils';

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
