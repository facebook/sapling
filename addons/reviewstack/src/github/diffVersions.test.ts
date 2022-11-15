/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitChange} from './diffTypes';

import {
  depthFirstPathCompare,
  diffVersions,
  isStrictlyIncreasing,
  splitOffFirstPathComponent,
} from './diffVersions';

describe('diffVersions', () => {
  test('combines changes for the same file', () => {
    const beforeDiff = [createAddChange('foo')];
    const afterDiff = [createModifyChange('foo')];
    const diff = diffVersions(beforeDiff, afterDiff);

    expect(diff.length).toBe(1);
    expect(diff[0].type).toBe('modify');
  });

  test('removes changes that are the same', () => {
    const modifyFoo = createModifyChange('foo');
    const beforeDiff = [modifyFoo];
    const afterDiff = [modifyFoo];
    const diff = diffVersions(beforeDiff, afterDiff);

    expect(diff.length).toBe(0);
  });

  test('inverts changes that exist only in "before"', () => {
    const modifyFoo = createModifyChange('foo');
    const modifyFooBar = createModifyChange('foo/bar');
    const addQux = createAddChange('qux');
    const beforeDiff = [modifyFoo, addQux];
    const afterDiff = [modifyFooBar];
    const diff = diffVersions(beforeDiff, afterDiff);

    expect(diff[diff.length - 1].type).toEqual('remove');
  });

  test('does not invert changes that exist only in "after"', () => {
    const modifyFoo = createModifyChange('foo');
    const modifyFooBar = createModifyChange('foo/bar');
    const addQux = createAddChange('qux');
    const beforeDiff = [modifyFooBar];
    const afterDiff = [modifyFoo, addQux];
    const diff = diffVersions(beforeDiff, afterDiff);

    expect(diff[diff.length - 1]).toEqual(addQux);
  });

  test('merges and returns changes in order', () => {
    const fooAbc = createModifyChange('foo/abc');
    const fooBarQux = createModifyChange('foo/bar/qux');
    const fooBaz = createModifyChange('foo/baz');
    const xyz = createModifyChange('xyz');

    const beforeDiff = [fooBarQux, xyz];
    const afterDiff = [fooAbc, fooBaz];
    const diff = diffVersions(beforeDiff, afterDiff);

    expect(isStrictlyIncreasing(diff)).toBe(true);
  });

  test('does not throw if sorted', () => {
    const aaa = createModifyChange('aaa');
    const bbb = createModifyChange('bbb');
    const ccc = createModifyChange('ccc');
    const sorted = [aaa, bbb, ccc];

    expect(() => {
      diffVersions(sorted, sorted);
    }).not.toThrowError();
  });

  test('throws if not sorted', () => {
    const aaa = createModifyChange('aaa');
    const bbb = createModifyChange('bbb');
    const zzz = createModifyChange('zzz');
    const unsorted = [bbb, zzz, aaa];

    expect(() => {
      diffVersions(unsorted, unsorted);
    }).toThrowError('diffs are not sorted');
  });
});

test('depthFirstPathCompare', () => {
  expect(depthFirstPathCompare('', '')).toBe('equal');
  expect(depthFirstPathCompare('foo', 'foo')).toBe('equal');
  expect(depthFirstPathCompare('foo/bar', 'foo/bar')).toBe('equal');

  expect(depthFirstPathCompare('', 'a')).toBe('less');
  expect(depthFirstPathCompare('a', 'b')).toBe('less');
  expect(depthFirstPathCompare('B', 'a')).toBe('less');
  expect(depthFirstPathCompare('aa', 'b')).toBe('less');
  expect(depthFirstPathCompare('foo/bar', 'foo/bar/qux')).toBe('less');

  expect(depthFirstPathCompare('foo/baz', 'foo/bar/qux')).toBe('greater');
});

test('splitOffFirstPathComponent', () => {
  expect(splitOffFirstPathComponent('')).toEqual(['', '']);
  expect(splitOffFirstPathComponent('f')).toEqual(['f', '']);
  expect(splitOffFirstPathComponent('foo/bar')).toEqual(['foo', 'bar']);
  expect(splitOffFirstPathComponent('foo/bar/baz')).toEqual(['foo', 'bar/baz']);
});

test('isStrictlyIncreasing', () => {
  const fooAbc = createModifyChange('foo/abc');
  const fooBarQux = createModifyChange('foo/bar/qux');
  const fooBaz = createModifyChange('foo/baz');
  const fooBazXyz = createModifyChange('foo/baz/xyz');

  expect(isStrictlyIncreasing([fooAbc, fooBarQux, fooBaz])).toBe(true);
  expect(isStrictlyIncreasing([fooAbc, fooBaz])).toBe(true);
  expect(isStrictlyIncreasing([fooBaz, fooBazXyz])).toBe(true);
  expect(isStrictlyIncreasing([fooAbc])).toBe(true);

  expect(isStrictlyIncreasing([fooBaz, fooAbc, fooBarQux])).toBe(false);
  expect(isStrictlyIncreasing([fooBarQux, fooAbc])).toBe(false);
  expect(isStrictlyIncreasing([fooAbc, fooAbc])).toBe(false);
});

function createAddChange(path: string): CommitChange {
  const [basePath, name] = splitBasePathAndName(path);
  const oid = 'a'.repeat(40);

  return {
    type: 'add',
    basePath,
    entry: {
      oid,
      object: null,
      name,
      path,
      type: 'blob',
      mode: 0o100644,
    },
  };
}

function createModifyChange(path: string): CommitChange {
  const [basePath, name] = splitBasePathAndName(path);
  const beforeOid = 'a'.repeat(40);
  const afterOid = 'b'.repeat(40);

  return {
    type: 'modify',
    basePath,
    before: {
      oid: beforeOid,
      object: null,
      name,
      path,
      type: 'blob',
      mode: 0o100644,
    },
    after: {
      oid: afterOid,
      object: null,
      name,
      path,
      type: 'blob',
      mode: 0o100644,
    },
  };
}

function splitBasePathAndName(path: string): [string, string] {
  const index = path.lastIndexOf('/');
  if (index === -1) {
    return ['', path];
  } else {
    return [path.slice(0, index), path.slice(index + 1)];
  }
}
