/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {FileRev} from '../fileStackState';

import {FileStackState, Source} from '../fileStackState';

describe('FileStackState', () => {
  const commonContents = ['b\nc\nd\n', 'a\nb\nc\nd\n', 'a\nb\nc\nd\ne\n', 'a\nc\nd\ne\n'];
  const revLength = commonContents.length as FileRev;

  it('converts between formats', () => {
    const stack = new FileStackState(commonContents);
    const formats = [
      (s: FileStackState) =>
        new FileStackState(Source({type: 'plain', value: s.convertToPlainText(), revLength})),
      (s: FileStackState) =>
        new FileStackState(Source({type: 'linelog', value: s.convertToLineLog(), revLength})),
      (s: FileStackState) =>
        new FileStackState(Source({type: 'flatten', value: s.convertToFlattenLines(), revLength})),
    ];
    formats.forEach(fromFormat => {
      const fromState = fromFormat(stack);
      formats.forEach(toFormat => {
        const toState = toFormat(fromState);
        expect(toState.revs()).toStrictEqual([...commonContents.keys()]);
        expect(toState.revs().map(rev => toState.getRev(rev))).toStrictEqual(commonContents);
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
        [1, new Set([0])],
        [2, new Set([0])],
      ]),
    );
  });

  it('supports blame', () => {
    const stack = new FileStackState(commonContents);
    expect(stack.blame(0 as FileRev)).toStrictEqual([0, 0, 0]);
    expect(stack.blame(2 as FileRev)).toStrictEqual([1, 0, 0, 0, 2]);
  });

  it('supports editing text without affecting the stack', () => {
    const stack = new FileStackState(commonContents).editText(0 as FileRev, 'b\nC\nD\n', false);
    expect(stack.getRev(0 as FileRev)).toBe('b\nC\nD\n');
    expect(stack.getRev(1 as FileRev)).toBe('a\nb\nc\nd\n');
  });

  it('supports editing text and updating the stack', () => {
    const stack = new FileStackState(commonContents).editText(0 as FileRev, 'b\nC\nD\n', true);
    expect(stack.getRev(0 as FileRev)).toBe('b\nC\nD\n');
    expect(stack.getRev(1 as FileRev)).toBe('a\nb\nC\nD\n');
  });

  it('supports editing chunk at the given rev', () => {
    // Edit rev 1 from rev 0's line ranges.
    const stack = new FileStackState(commonContents).editChunk(0 as FileRev, 1, 3, 1 as FileRev, [
      'C\n',
      'D\n',
    ]);
    // rev 0 is not changed.
    expect(stack.getRev(0 as FileRev)).toBe('b\nc\nd\n');
    // rev 1 is edited.
    expect(stack.getRev(1 as FileRev)).toBe('a\nb\nC\nD\n');
  });

  it('supports remapping revs', () => {
    const stack = new FileStackState(['a\n', 'a\nb\n', 'z\na\nb\n']).remapRevs(
      new Map([
        [1 as FileRev, 2 as FileRev],
        [2 as FileRev, 1 as FileRev],
      ]),
    );
    expect(stack.getRev(1 as FileRev)).toBe('z\na\n');
    expect(stack.getRev(2 as FileRev)).toBe('z\na\nb\n');
  });

  it('supports moving lines between revs', () => {
    let stack = new FileStackState(commonContents);
    // Move +a from rev 1 to rev 2 (->).
    stack = stack.moveLines(1 as FileRev, 0, 1, [], [1 as FileRev]);
    expect(stack.getRev(1 as FileRev)).toBe('b\nc\nd\n');
    // Move -b from rev 3 (present in rev 2) to rev 2 (present in rev 1) (<-).
    stack = stack.moveLines(2 as FileRev, 1, 2, [], [2 as FileRev]);
    expect(stack.getRev(2 as FileRev)).toBe('a\nc\nd\ne\n');
    // Move +e from rev 2 to rev 1 (<-).
    stack = stack.moveLines(2 as FileRev, 3, 4, [1 as FileRev], []);
    expect(stack.getRev(1 as FileRev)).toBe('b\nc\nd\ne\n');
    expect(stack.convertToPlainText().toArray()).toStrictEqual([
      'b\nc\nd\n',
      'b\nc\nd\ne\n',
      'a\nc\nd\ne\n',
      'a\nc\nd\ne\n',
    ]);
  });

  it('supports appending text', () => {
    let stack = new FileStackState([]);
    expect(stack.source.revLength).toBe(0);
    stack = stack.editText(0 as FileRev, 'a', false);
    expect(stack.source.revLength).toBe(1);
    stack = stack.editText(1 as FileRev, 'b', false);
    expect(stack.source.revLength).toBe(2);
    stack = stack.editText(2 as FileRev, 'c', true);
    expect(stack.source.revLength).toBe(3);
    expect(stack.getRev(2 as FileRev)).toBe('c');
  });
});
