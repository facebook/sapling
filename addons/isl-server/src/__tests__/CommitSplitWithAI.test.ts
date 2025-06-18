/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {reduceContextualLines} from '../facebook/phabricator/CommitSplitWithAI';
import type {PhabricatorAiDiffSplitCommitDiffFileLine} from '../facebook/phabricator/generated/graphql';

describe('reduceContextualLines', () => {
  // Test case 1: Empty array should return empty array
  test('empty array returns empty array', () => {
    const lines: PhabricatorAiDiffSplitCommitDiffFileLine[] = [];
    const result = reduceContextualLines(lines);
    expect(result).toEqual([]);
  });

  // Test case 2: No changed lines should return empty array
  test('no changed lines returns empty array', () => {
    const lines: PhabricatorAiDiffSplitCommitDiffFileLine[] = [
      {a: 1, b: 1, content: 'line 1'},
      {a: 2, b: 2, content: 'line 2'},
      {a: 3, b: 3, content: 'line 3'},
    ];
    const result = reduceContextualLines(lines);
    expect(result).toEqual([]);
  });

  // Test case 3: Single changed line should return the changed line and context lines
  test('single changed line returns the changed line and context lines', () => {
    const lines: PhabricatorAiDiffSplitCommitDiffFileLine[] = [
      {a: 1, b: 1, content: 'line 1'},
      {a: 2, b: 2, content: 'line 2'},
      {a: 3, b: 3, content: 'line 3'},
      {a: 4, b: null, content: 'line 4 removed'},
      {a: 5, b: 4, content: 'line 5'},
      {a: 6, b: 5, content: 'line 6'},
      {a: 7, b: 6, content: 'line 7'},
      {a: 8, b: 7, content: 'line 8'},
    ];

    // With default maxContextLines = 3
    const result = reduceContextualLines(lines);
    expect(result).toEqual([
      {a: 1, b: 1, content: 'line 1'},
      {a: 2, b: 2, content: 'line 2'},
      {a: 3, b: 3, content: 'line 3'},
      {a: 4, b: null, content: 'line 4 removed'},
      {a: 5, b: 4, content: 'line 5'},
      {a: 6, b: 5, content: 'line 6'},
      {a: 7, b: 6, content: 'line 7'},
    ]);

    // With maxContextLines = 1
    const resultWithLessContext = reduceContextualLines(lines, 1);
    expect(resultWithLessContext).toEqual([
      {a: 3, b: 3, content: 'line 3'},
      {a: 4, b: null, content: 'line 4 removed'},
      {a: 5, b: 4, content: 'line 5'},
    ]);
  });

  // Test case 4: Multiple changed lines should return all changed lines and context lines
  test('multiple changed lines returns all changed lines and context lines', () => {
    const lines: PhabricatorAiDiffSplitCommitDiffFileLine[] = [
      {a: 1, b: 1, content: 'line 1'},
      {a: 2, b: 2, content: 'line 2'},
      {a: 3, b: null, content: 'line 3 removed'},
      {a: 4, b: 3, content: 'line 4'},
      {a: 5, b: 4, content: 'line 5'},
      {a: 6, b: 5, content: 'line 6'},
      {a: 7, b: 6, content: 'line 7'},
      {a: 8, b: 7, content: 'line 8'},
      {a: null, b: 8, content: 'line 9 added'},
      {a: 9, b: 9, content: 'line 10'},
      {a: 10, b: 10, content: 'line 11'},
      {a: 11, b: 11, content: 'line 12'},
    ];

    // With default maxContextLines = 3
    const result = reduceContextualLines(lines);

    // All lines should be included because the changed lines are close enough
    expect(result).toEqual(lines);

    // With maxContextLines = 1
    const resultWithLessContext = reduceContextualLines(lines, 1);
    expect(resultWithLessContext).toEqual([
      {a: 2, b: 2, content: 'line 2'},
      {a: 3, b: null, content: 'line 3 removed'},
      {a: 4, b: 3, content: 'line 4'},
      {a: 8, b: 7, content: 'line 8'},
      {a: null, b: 8, content: 'line 9 added'},
      {a: 9, b: 9, content: 'line 10'},
    ]);
  });

  // Test case 5: Changed lines far apart should only include context around each change
  test('changed lines far apart only include context around each change', () => {
    const lines: PhabricatorAiDiffSplitCommitDiffFileLine[] = [
      {a: 1, b: 1, content: 'line 1'},
      {a: 2, b: null, content: 'line 2 removed'},
      {a: 3, b: 2, content: 'line 3'},
      {a: 4, b: 3, content: 'line 4'},
      {a: 5, b: 4, content: 'line 5'},
      {a: 6, b: 5, content: 'line 6'},
      {a: 7, b: 6, content: 'line 7'},
      {a: 8, b: 7, content: 'line 8'},
      {a: 9, b: 8, content: 'line 9'},
      {a: 10, b: 9, content: 'line 10'},
      {a: 11, b: 10, content: 'line 11'},
      {a: 12, b: 11, content: 'line 12'},
      {a: 13, b: null, content: 'line 13 removed'},
      {a: 14, b: 12, content: 'line 14'},
      {a: 15, b: 13, content: 'line 15'},
    ];

    // With maxContextLines = 2
    const result = reduceContextualLines(lines, 2);
    expect(result).toEqual([
      {a: 1, b: 1, content: 'line 1'},
      {a: 2, b: null, content: 'line 2 removed'},
      {a: 3, b: 2, content: 'line 3'},
      {a: 4, b: 3, content: 'line 4'},
      {a: 11, b: 10, content: 'line 11'},
      {a: 12, b: 11, content: 'line 12'},
      {a: 13, b: null, content: 'line 13 removed'},
      {a: 14, b: 12, content: 'line 14'},
      {a: 15, b: 13, content: 'line 15'},
    ]);
  });
});
