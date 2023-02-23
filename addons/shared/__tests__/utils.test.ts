/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {basename, mapObject} from '../utils';

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
