/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import splitPath from './splitPath';

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
