/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {FileStackState} from '../fileStackState';
import {describe, it, expect} from '@jest/globals';

describe('FileStackState', () => {
  const commonContents = ['b\nc\nd\n', 'a\nb\nc\nd\n', 'a\nb\nc\nd\ne\n', 'a\nc\nd\ne\n'];

  it('converts between formats', () => {
    const stack = new FileStackState(commonContents);
    const formats = [
      () => stack.convertToPlainText(),
      () => stack.convertToLineLog(),
      () => stack.convertToFlattenLines(),
    ];
    formats.forEach(fromFormat => {
      fromFormat();
      formats.forEach(toFormat => {
        toFormat();
        expect(stack.revs()).toStrictEqual([...commonContents.keys()]);
        expect(stack.revs().map(rev => stack.get(rev))).toStrictEqual(commonContents);
      });
    });
  });

  // Some features are thin wrappers around linelog. Their tests overlap
  // with linelog tests. We prefer corner cases to be tested at the
  // bottom layer. If you'd like to add more corner cases, consider
  // adding them in linelog.test.ts.

  it('analyses dependency', () => {
    const stack = new FileStackState(['b\n', 'a\nb\n', 'a\nb\nc\n']);
    expect(stack.calculateDepMap()).toStrictEqual(
      new Map([
        [0, new Set()],
        [1, new Set([0])],
        [2, new Set([0])],
      ]),
    );
  });

  it('supports blame', () => {
    const stack = new FileStackState(commonContents);
    expect(stack.blame(0)).toStrictEqual([0, 0, 0]);
    expect(stack.blame(2)).toStrictEqual([1, 0, 0, 0, 2]);
  });

  it('supports editing text without affecting the stack', () => {
    const stack = new FileStackState(commonContents);
    stack.editText(0, 'b\nC\nD\n', false);
    expect(stack.get(0)).toBe('b\nC\nD\n');
    expect(stack.get(1)).toBe('a\nb\nc\nd\n');
  });

  it('supports editing text and updating the stack', () => {
    const stack = new FileStackState(commonContents);
    stack.editText(0, 'b\nC\nD\n', true);
    expect(stack.get(0)).toBe('b\nC\nD\n');
    expect(stack.get(1)).toBe('a\nb\nC\nD\n');
  });

  it('supports editing chunk at the given rev', () => {
    const stack = new FileStackState(commonContents);
    // Edit rev 1 from rev 0's line ranges.
    stack.editChunk(0, 1, 3, 1, ['C\n', 'D\n']);
    // rev 0 is not changed.
    expect(stack.get(0)).toBe('b\nc\nd\n');
    // rev 1 is edited.
    expect(stack.get(1)).toBe('a\nb\nC\nD\n');
  });

  it('supports remapping revs', () => {
    const stack = new FileStackState(['a\n', 'a\nb\n', 'z\na\nb\n']);
    stack.remapRevs(
      new Map([
        [1, 2],
        [2, 1],
      ]),
    );
    expect(stack.get(1)).toBe('z\na\n');
    expect(stack.get(2)).toBe('z\na\nb\n');
  });

  it('supports moving lines between revs', () => {
    const stack = new FileStackState(commonContents);
    // Move +a from rev 1 to rev 2 (->).
    stack.moveLines(1, 0, 1, [], [1]);
    expect(stack.get(1)).toBe('b\nc\nd\n');
    // Move -b from rev 3 (present in rev 2) to rev 2 (present in rev 1) (<-).
    stack.moveLines(2, 1, 2, [], [2]);
    expect(stack.get(2)).toBe('a\nc\nd\ne\n');
    // Move +e from rev 2 to rev 1 (<-).
    stack.moveLines(2, 3, 4, [1], []);
    expect(stack.get(1)).toBe('b\nc\nd\ne\n');
    expect(stack.convertToPlainText()).toStrictEqual([
      'b\nc\nd\n',
      'b\nc\nd\ne\n',
      'a\nc\nd\ne\n',
      'a\nc\nd\ne\n',
    ]);
  });
});
