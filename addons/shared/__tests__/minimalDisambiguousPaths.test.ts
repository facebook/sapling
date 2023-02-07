/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {minimalDisambiguousPaths} from '../minimalDisambiguousPaths';

describe('computeDisplayPaths', () => {
  it('should compute depth 1 paths correctly', () => {
    const input = ['/a/b.js', '/a/c.js'];
    const expected = ['b.js', 'c.js'];
    const actual = minimalDisambiguousPaths(input);
    expect(actual).toEqual(expected);
  });

  it('should compute depth 2 paths correctly', () => {
    const input = ['/a/b.js', '/c/b.js'];
    const expected = ['/a/b.js', '/c/b.js'];
    const actual = minimalDisambiguousPaths(input);
    expect(actual).toEqual(expected);
  });

  it('should compute a mix of depths 1 to 5 paths correctly', () => {
    const input = [
      '/a/b/c/d/e.js',
      '/z/b/c/d/e.js',
      'a/y/z/e.js',
      'a/y/e.js',
      'a/z/e.js',
      'a/f.js',
    ];
    const expected = ['/a/b/c/d/e.js', '/z/b/c/d/e.js', 'y/z/e.js', 'y/e.js', 'a/z/e.js', 'f.js'];
    const actual = minimalDisambiguousPaths(input);
    expect(actual).toEqual(expected);
  });

  it('should respect alwaysShowLeadingSeparator = false', () => {
    const input = [
      '/a/b/c/d/e.js',
      '/z/b/c/d/e.js',
      'a/y/z/e.js',
      'a/y/e.js',
      'a/z/e.js',
      'a/f.js',
    ];
    const expected = ['/a/b/c/d/e.js', '/z/b/c/d/e.js', 'y/z/e.js', 'y/e.js', 'a/z/e.js', 'f.js'];
    const actual = minimalDisambiguousPaths(input, {alwaysShowLeadingSeparator: false});
    expect(actual).toEqual(expected);
  });

  it('should handle duplicate paths correctly', () => {
    const input = ['/a/b/c/d/e.js', '/a/b/c/d/e.js'];
    const expected = ['/a/b/c/d/e.js', '/a/b/c/d/e.js'];
    const actual = minimalDisambiguousPaths(input);
    expect(actual).toEqual(expected);
  });

  it('should handle paths that appear as a subpath of another path correctly', () => {
    const input = ['/a/b/c/d/e.js', '/c/d/e.js'];
    const expected = ['/b/c/d/e.js', '/c/d/e.js'];
    const actual = minimalDisambiguousPaths(input);
    expect(actual).toEqual(expected);
  });

  it('should honor the max depth argument', () => {
    const input = ['/a/b/c/d/e.js', '/z/b/c/d/e.js'];
    const expected = ['/d/e.js', '/d/e.js'];
    const actual = minimalDisambiguousPaths(input, {maxDepth: 2});
    expect(actual).toEqual(expected);
  });

  it('should handle empty input', () => {
    const input: string[] = [];
    const expected: string[] = [];
    const actual = minimalDisambiguousPaths(input);
    expect(actual).toEqual(expected);
  });

  it('should handle a realistic scenario path separators', () => {
    const input = [
      '/data/users/x/fbsource/fbcode/foo/bar.js',
      '/data/users/x/fbsource/fbcode/bar/bar.js',
      '/Users/y/fbsource/fbcode/foo/bar.js',
      '/Users/y/fbsource/fbcode/bar/bar.js',
    ];
    const expected = [
      '/x/fbsource/fbcode/foo/bar.js',
      '/x/fbsource/fbcode/bar/bar.js',
      '/y/fbsource/fbcode/foo/bar.js',
      '/y/fbsource/fbcode/bar/bar.js',
    ];
    const actual = minimalDisambiguousPaths(input);
    expect(actual).toEqual(expected);
  });

  it('should handle root correctly (maintain the "/")', () => {
    const input = ['/', '/foo'];
    const expected = ['/', 'foo'];
    const actual = minimalDisambiguousPaths(input);
    expect(actual).toEqual(expected);
  });

  it('should handle relative paths correctly (not insert a new "/")', () => {
    const input = ['hello/hi.txt', 'baz/bar.js', 'foo/bar.js'];
    const expected = ['hi.txt', 'baz/bar.js', 'foo/bar.js'];
    const actual = minimalDisambiguousPaths(input);
    expect(actual).toEqual(expected);
  });

  it('should handle trailing slashes correctly', () => {
    const input = ['/foo/'];
    const expected = ['foo'];
    const actual = minimalDisambiguousPaths(input);
    expect(actual).toEqual(expected);
  });

  it('should handle windows paths without ambiguity correctly', () => {
    const input = ['c:\\foo\\baz.js', 'c:\\foo\\bar.js'];
    const expected = ['baz.js', 'bar.js'];
    const actual = minimalDisambiguousPaths(input);
    expect(actual).toEqual(expected);
  });

  it('should handle windows paths with ambiguity correctly', () => {
    const input = ['c:\\foo\\bar\\baz.js', 'c:\\foo\\bar2\\baz.js'];
    const expected = ['\\bar\\baz.js', '\\bar2\\baz.js'];
    const actual = minimalDisambiguousPaths(input);
    expect(actual).toEqual(expected);
  });

  it('should include the prefix on windows when it is showing the full path', () => {
    const input = ['c:\\foo2\\bar.js', 'c:\\foo\\bar.js'];
    const expected = ['c:\\foo2\\bar.js', 'c:\\foo\\bar.js'];
    const actual = minimalDisambiguousPaths(input);
    expect(actual).toEqual(expected);
  });

  it('should hide windows path drive prefix when paths are minimized', () => {
    const input = ['c:\\foo\\bar\\boo.js', 'c:\\foo\\bar2\\blam.js'];
    const expected = ['boo.js', 'blam.js'];
    const actual = minimalDisambiguousPaths(input);
    expect(actual).toEqual(expected);
  });
});
