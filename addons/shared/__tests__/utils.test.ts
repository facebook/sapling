/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {basename, generatorContains, group, mapObject, partition, truncate} from '../utils';

describe('basename', () => {
  it('/path/to/foo.txt -> foo.txt', () => {
    expect(basename('/path/to/foo.txt')).toEqual('foo.txt');
  });

  it("/path/to/ -> ''", () => {
    expect(basename('/path/to/')).toEqual('');
  });

  it('customizable delimeters', () => {
    expect(basename('/path/to/foo.txt', '.')).toEqual('txt');
  });

  it('empty string', () => {
    expect(basename('')).toEqual('');
  });

  it('delimeter not in string', () => {
    expect(basename('hello world')).toEqual('hello world');
  });
});

describe('mapObject', () => {
  it('maps object types', () => {
    expect(mapObject({foo: 123, bar: 456}, ([key, value]) => [key, {value}])).toEqual({
      foo: {value: 123},
      bar: {value: 456},
    });
  });

  it('handles different key types', () => {
    expect(mapObject({foo: 123, bar: 456}, ([key, value]) => [value, key])).toEqual({
      123: 'foo',
      456: 'bar',
    });
  });
});

describe('generatorContains', () => {
  let lastYield = 0;
  function* gen() {
    lastYield = 3;
    yield 3;
    lastYield = 5;
    yield 5;
  }

  it('supports testing a value', () => {
    expect(generatorContains(gen(), 3)).toBeTruthy();
    expect(lastYield).toBe(3);
    expect(generatorContains(gen(), 5)).toBeTruthy();
    expect(lastYield).toBe(5);
    expect(generatorContains(gen(), 2)).toBeFalsy();
    expect(lastYield).toBe(5);
  });

  it('supports testing via a function', () => {
    expect(generatorContains(gen(), v => v > 2)).toBeTruthy();
    expect(lastYield).toBe(3);
    expect(generatorContains(gen(), v => v > 4)).toBeTruthy();
    expect(lastYield).toBe(5);
    expect(generatorContains(gen(), v => v > 6)).toBeFalsy();
    expect(lastYield).toBe(5);
  });
});

describe('truncate', () => {
  it('does not truncate strings within the maxLength constraint', () => {
    expect(truncate('abc', 3)).toBe('abc');
    expect(truncate('def', 4)).toBe('def');
  });

  it('truncates long strings', () => {
    expect(truncate('abc', 2)).toBe('a…');
    expect(truncate('def', 0)).toBe('…');
  });
});

describe('partition', () => {
  it('partitions an array into two arrays based on a predicate', () => {
    expect(partition([1, 2, 3], n => n % 2 === 0)).toEqual([[2], [1, 3]]);
    expect(partition([1, 2, 3], () => true)).toEqual([[1, 2, 3], []]);
    expect(partition([1, 2, 3], () => false)).toEqual([[], [1, 2, 3]]);
  });
});

describe('group', () => {
  it('groups an array into multiple subarrays based on a key', () => {
    expect(group(['a', 'ab', 'abc', 'ef', 'g'], e => e.length)).toEqual({
      1: ['a', 'g'],
      2: ['ab', 'ef'],
      3: ['abc'],
    });
  });
});
