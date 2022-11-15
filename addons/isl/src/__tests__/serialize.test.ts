/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {deserialize, serialize} from '../serialize';

describe('serialize', () => {
  it('map', () => {
    const map = new Map([
      [1, 'a'],
      [2, 'b'],
    ]);
    expect(deserialize(serialize(map))).toEqual(map);
  });
  it('set', () => {
    const set = new Set([1, 2, 3, 1]);
    expect(deserialize(serialize(set))).toEqual(set);
  });

  it('nesting Maps and Sets', () => {
    const complex = new Map([
      ['a', new Set([1, 2, 3, 1])],
      ['b', new Set([10, 20, 10])],
    ]);
    expect(deserialize(serialize(complex))).toEqual(complex);
  });

  it('Dates', () => {
    const date = new Date('2020-12-01');
    expect(deserialize(serialize(date))?.valueOf()).toBe(date.valueOf());
  });

  it('nested objects', () => {
    const nested = {
      a: new Date('2020-12-01'),
      b: [new Set([1, 2, 3, 1]), new Set([2, 3, 4, 5])],
      c: {
        d: new Map([
          ['1', 1],
          ['2', 2],
        ]),
        e: 'just a regular primitive',
        f: 2,
        g: [
          {
            a: new Set([1, 2, 3, 4]),
          },
          {a: new Set([1, 2, 3, 4])},
        ],
      },
    };
    expect(deserialize(serialize(nested))).toEqual(nested);
  });

  it('undefined is preserved', () => {
    expect(deserialize(serialize(undefined))).toEqual(undefined);
  });

  it('objects without a prototype', () => {
    const o = Object.create(null);
    o.a = 123;
    expect(deserialize(serialize(o))).toEqual({a: 123});
  });

  it('errors', () => {
    expect(deserialize(serialize(new Error('this is my error\nwow')))).toEqual(
      new Error('this is my error\nwow'),
    );
  });
});
