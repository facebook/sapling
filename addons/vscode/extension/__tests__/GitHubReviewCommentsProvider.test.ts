/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {
  reviewCommentBody,
  selectedMultilineRanges,
  suggestionBody,
} from '../GitHubReviewCommentsProvider';

describe('GitHub review suggestions', () => {
  it('copies selected lines into a GitHub suggestion block', () => {
    expect(suggestionBody('const one = 1;\nconst two = 2;')).toBe(
      '```suggestion\nconst one = 1;\nconst two = 2;\n```',
    );
  });

  it('preserves an existing comment before the suggestion', () => {
    expect(suggestionBody('return result;', 'Please simplify this.')).toBe(
      'Please simplify this.\n\n```suggestion\nreturn result;\n```',
    );
  });
});

describe('GitHub review comment display', () => {
  it('puts a remote comment link above the body', () => {
    expect(reviewCommentBody('Looks good.', 'https://github.com/o/r/pull/1#discussion_r2')).toBe(
      `[View on GitHub](https://github.com/o/r/pull/1#discussion_r2)\n\nLooks good.`,
    );
  });
});

describe('active multiline comment range', () => {
  it('marks every fully or partially selected line', () => {
    expect(
      selectedMultilineRanges([{isEmpty: false, start: {line: 4}, end: {line: 7, character: 3}}]),
    ).toEqual([{startLine: 4, endLine: 7}]);
  });

  it('does not include the next line when the selection ends at column zero', () => {
    expect(
      selectedMultilineRanges([{isEmpty: false, start: {line: 4}, end: {line: 7, character: 0}}]),
    ).toEqual([{startLine: 4, endLine: 6}]);
  });

  it('does not mark a cursor or a single-line selection', () => {
    expect(
      selectedMultilineRanges([
        {isEmpty: true, start: {line: 4}, end: {line: 4, character: 0}},
        {isEmpty: false, start: {line: 5}, end: {line: 5, character: 8}},
      ]),
    ).toEqual([]);
  });
});
