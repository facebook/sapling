/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {HighlightedToken} from './textmate/tokenizeFileContents';

import {
  createTokenizedIntralineDiff,
  MAX_INPUT_LENGTH_FOR_INTRALINE_DIFF,
} from './createTokenizedIntralineDiff';

describe('createTokenizedIntralineDiff', () => {
  test('empty string', () => {
    const beforeLine = '';
    const beforeTokens: HighlightedToken[] = [];
    const afterLine = '';
    const afterTokens: HighlightedToken[] = [];
    const [left, right] = createTokenizedIntralineDiff(
      beforeLine,
      beforeTokens,
      afterLine,
      afterTokens,
    );
    expect(left).toBe(null);
    expect(right).toBe(null);
  });

  test('diff on words, not chars', () => {
    const beforeLine = 'global x';
    const beforeTokens = [
      {start: 0, end: 6, color: 2},
      {start: 6, end: 7, color: 1},
      {start: 7, end: 8, color: 3},
    ];
    const afterLine = 'nonlocal xyz';
    const afterTokens = [
      {start: 0, end: 8, color: 2},
      {start: 8, end: 9, color: 1},
      {start: 9, end: 12, color: 3},
    ];
    const [left, right] = createTokenizedIntralineDiff(
      beforeLine,
      beforeTokens,
      afterLine,
      afterTokens,
    );
    expect(left).toEqual([
      <span key={0} className="mtk2 patch-remove-word patch-word-begin patch-word-end">
        global
      </span>,
      <span key={6} className="mtk1">
        {' '}
      </span>,
      <span key={7} className="mtk3 patch-remove-word patch-word-begin patch-word-end">
        x
      </span>,
    ]);
    expect(right).toEqual([
      <span key={0} className="mtk2 patch-add-word patch-word-begin patch-word-end">
        nonlocal
      </span>,
      <span key={8} className="mtk1">
        {' '}
      </span>,
      <span key={9} className="mtk3 patch-add-word patch-word-begin patch-word-end">
        xyz
      </span>,
    ]);
  });

  test('renamed variable', () => {
    const beforeLine = 'func _unhandled_input(event: InputEvent) -> void:';
    const beforeTokens = [
      {start: 0, end: 4, color: 4},
      {start: 4, end: 5, color: 1},
      {start: 5, end: 21, color: 10},
      {start: 21, end: 29, color: 1},
      {start: 29, end: 39, color: 9},
      {start: 39, end: 44, color: 1},
      {start: 44, end: 48, color: 9},
      {start: 48, end: 49, color: 1},
    ];
    const afterLine = 'func _unhandled_input(input_event: InputEvent) -> void:';
    const afterTokens = [
      {start: 0, end: 4, color: 4},
      {start: 4, end: 5, color: 1},
      {start: 5, end: 21, color: 10},
      {start: 21, end: 35, color: 1},
      {start: 35, end: 45, color: 9},
      {start: 45, end: 50, color: 1},
      {start: 50, end: 54, color: 9},
      {start: 54, end: 55, color: 1},
    ];
    const [left, right] = createTokenizedIntralineDiff(
      beforeLine,
      beforeTokens,
      afterLine,
      afterTokens,
    );
    expect(left).toEqual([
      <span key={0} className="mtk4">
        func
      </span>,
      <span key={4} className="mtk1">
        {' '}
      </span>,
      <span key={5} className="mtk10">
        _unhandled_input
      </span>,
      <span key={21} className="mtk1">
        (
      </span>,
      <span key={22} className="mtk1 patch-remove-word patch-word-begin patch-word-end">
        event
      </span>,
      <span key={27} className="mtk1">
        {': '}
      </span>,
      <span key={29} className="mtk9">
        InputEvent
      </span>,
      <span key={39} className="mtk1">
        {') -> '}
      </span>,
      <span key={44} className="mtk9">
        void
      </span>,
      <span key={48} className="mtk1">
        :
      </span>,
    ]);
    expect(right).toEqual([
      <span key={0} className="mtk4">
        func
      </span>,
      <span key={4} className="mtk1">
        {' '}
      </span>,
      <span key={5} className="mtk10">
        _unhandled_input
      </span>,
      <span key={21} className="mtk1">
        (
      </span>,
      <span key={22} className="mtk1 patch-add-word patch-word-begin patch-word-end">
        input_event
      </span>,
      <span key={33} className="mtk1">
        {': '}
      </span>,
      <span key={35} className="mtk9">
        InputEvent
      </span>,
      <span key={45} className="mtk1">
        {') -> '}
      </span>,
      <span key={50} className="mtk9">
        void
      </span>,
      <span key={54} className="mtk1">
        :
      </span>,
    ]);
  });

  test('diffing unrelated lines', () => {
    const beforeLine = '# - event: Triggered event';
    const beforeTokens = [{start: 0, end: 26, color: 3}];
    const afterLine = 'func _unhandled_input(input_event: InputEvent) -> void:';
    const afterTokens = [
      {start: 0, end: 4, color: 4},
      {start: 4, end: 5, color: 1},
      {start: 5, end: 21, color: 10},
      {start: 21, end: 35, color: 1},
      {start: 35, end: 45, color: 9},
      {start: 45, end: 50, color: 1},
      {start: 50, end: 54, color: 9},
      {start: 54, end: 55, color: 1},
    ];
    const [left, right] = createTokenizedIntralineDiff(
      beforeLine,
      beforeTokens,
      afterLine,
      afterTokens,
    );
    expect(left).toEqual([
      <span key={0} className="mtk3 patch-remove-word patch-word-begin patch-word-end">
        {'#'}
      </span>,
      <span key={1} className="mtk3">
        {' '}
      </span>,
      <span key={2} className="mtk3 patch-remove-word patch-word-begin patch-word-end">
        {'- event'}
      </span>,
      <span key={9} className="mtk3">
        {': '}
      </span>,
      <span key={11} className="mtk3 patch-remove-word patch-word-begin patch-word-end">
        {'Triggered'}
      </span>,
      <span key={20} className="mtk3">
        {' '}
      </span>,
      <span key={21} className="mtk3 patch-remove-word patch-word-begin patch-word-end">
        {'event'}
      </span>,
    ]);
    expect(right).toEqual([
      <span key={0} className="mtk4 patch-add-word patch-word-begin patch-word-end">
        func
      </span>,
      <span key={4} className="mtk1">
        {' '}
      </span>,
      <span key={5} className="mtk10 patch-add-word patch-word-begin">
        _unhandled_input
      </span>,
      <span key={21} className="mtk1 patch-add-word patch-word-end">
        {'(input_event'}
      </span>,
      <span key={33} className="mtk1">
        {': '}
      </span>,
      <span key={35} className="mtk9 patch-add-word patch-word-begin">
        InputEvent
      </span>,
      <span key={45} className="mtk1 patch-add-word patch-word-end">
        {')'}
      </span>,
      <span key={46} className="mtk1">
        {' '}
      </span>,
      <span key={47} className="mtk1 patch-add-word patch-word-begin">
        {'-> '}
      </span>,
      <span key={50} className="mtk9 patch-add-word">
        void
      </span>,
      <span key={54} className="mtk1 patch-add-word patch-word-end">
        :
      </span>,
    ]);
  });

  test('first patch-remove-work chunk has multiple tokens', () => {
    const beforeLine = '    return true;';
    const beforeTokens = [
      {start: 0, end: 4, color: 1},
      {start: 4, end: 10, color: 4},
      {start: 10, end: 11, color: 1},
      {start: 11, end: 15, color: 9},
      {start: 15, end: 16, color: 1},
    ];
    const afterLine = '';
    const afterTokens = [{start: 0, end: 0, color: 1}];
    const [left, right] = createTokenizedIntralineDiff(
      beforeLine,
      beforeTokens,
      afterLine,
      afterTokens,
    );
    expect(left).toEqual([
      <span key={0} className="mtk1 patch-remove-word patch-word-begin">
        {'    '}
      </span>,
      <span key={4} className="mtk4 patch-remove-word">
        return
      </span>,
      <span key={10} className="mtk1 patch-remove-word">
        {' '}
      </span>,
      <span key={11} className="mtk9 patch-remove-word">
        true
      </span>,
      <span key={15} className="mtk1 patch-remove-word patch-word-end">
        ;
      </span>,
    ]);
    expect(right).toEqual([]);
  });

  test('creating an intraline diff for a line that is too long should bail out', () => {
    // We repeat a piece of text with a space because createTokenizedIntralineDiff()
    // uses diffWords() under the hood.
    const beforeLine = 'reviewstack '.repeat(12);
    const beforeTokens = [{start: 0, end: beforeLine.length, color: 1}];
    const afterLineAtThreshold = 'reviewstack '.repeat(13);
    expect(beforeLine.length + afterLineAtThreshold.length).toBe(
      MAX_INPUT_LENGTH_FOR_INTRALINE_DIFF,
    );
    const afterTokensAtThreshold = [{start: 0, end: afterLineAtThreshold.length, color: 1}];
    const [leftAtThreshold, rightAtThreshold] = createTokenizedIntralineDiff(
      beforeLine,
      beforeTokens,
      afterLineAtThreshold,
      afterTokensAtThreshold,
    );
    expect(leftAtThreshold).toEqual([
      <span key={0} className="mtk1">
        {beforeLine}
      </span>,
    ]);
    expect(rightAtThreshold).toEqual([
      <span key={0} className="mtk1">
        {beforeLine}
      </span>,
      <span key={beforeLine.length} className="mtk1 patch-add-word patch-word-begin patch-word-end">
        {'reviewstack '}
      </span>,
    ]);

    // Verify that once the input exceeds the threshold, we no longer see CSS
    // classes like patch-word-begin or patch-word-end in the output, indicating
    // that the inline diff was not computed.
    const afterLineOneLonger = afterLineAtThreshold + 'X';
    expect(beforeLine.length + afterLineOneLonger.length).toBeGreaterThan(
      MAX_INPUT_LENGTH_FOR_INTRALINE_DIFF,
    );
    const afterTokensOneLonger = [{start: 0, end: afterLineOneLonger.length, color: 1}];
    const [leftOneLonger, rightOneLonger] = createTokenizedIntralineDiff(
      beforeLine,
      beforeTokens,
      afterLineOneLonger,
      afterTokensOneLonger,
    );
    expect(leftOneLonger).toEqual([
      <span key={0} className="mtk1">
        {beforeLine}
      </span>,
    ]);
    expect(rightOneLonger).toEqual([
      <span key={0} className="mtk1">
        {afterLineOneLonger}
      </span>,
    ]);
  });

  test('diffWordsWithSpace() must be used instead of diffWords() or content will be lost', () => {
    const beforeLine = '        <SplitDiffRow';
    const afterLine = '      path,';
    const beforeTokens = [
      {start: 0, end: 8, color: 1},
      {start: 8, end: 9, color: 13},
      {start: 9, end: 21, color: 9},
    ];
    const afterTokens = [
      {start: 0, end: 6, color: 1},
      {start: 6, end: 10, color: 14},
      {start: 10, end: 11, color: 1},
    ];
    const [left, right] = createTokenizedIntralineDiff(
      beforeLine,
      beforeTokens,
      afterLine,
      afterTokens,
    );
    expect(left).toEqual([
      <span key={0} className="mtk1 patch-remove-word patch-word-begin">
        {'        '}
      </span>,
      <span key={8} className="mtk13 patch-remove-word">
        {'<'}
      </span>,
      <span key={9} className="mtk9 patch-remove-word patch-word-end">
        {'SplitDiffRow'}
      </span>,
    ]);
    expect(right).toEqual([
      <span key={0} className="mtk1 patch-add-word patch-word-begin">
        {'      '}
      </span>,
      <span key={6} className="mtk14 patch-add-word">
        path
      </span>,
      <span key={10} className="mtk1 patch-add-word patch-word-end">
        {','}
      </span>,
    ]);
  });
});
