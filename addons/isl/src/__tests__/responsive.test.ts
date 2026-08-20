/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {readAtom, writeAtom} from '../jotaiUtils';
import {
  commitTreeWidth,
  mainContentWidthState,
  NARROW_COMMIT_TREE_WIDTH,
  NARROW_COMMIT_TREE_WIDTH_WHEN_COMPACT,
  renderCompactAtom,
  VERY_NARROW_COMMIT_TREE_WIDTH,
  VERY_NARROW_COMMIT_TREE_WIDTH_WHEN_COMPACT,
} from '../responsive';

describe('commitTreeWidth', () => {
  beforeEach(() => {
    writeAtom(renderCompactAtom, false);
  });

  it.each([
    [NARROW_COMMIT_TREE_WIDTH, 'wide'],
    [NARROW_COMMIT_TREE_WIDTH - 1, 'narrow'],
    [VERY_NARROW_COMMIT_TREE_WIDTH, 'narrow'],
    [VERY_NARROW_COMMIT_TREE_WIDTH - 1, 'very-narrow'],
  ] as const)('categorizes a width of %s as %s', (width, expected) => {
    writeAtom(mainContentWidthState, width);
    expect(readAtom(commitTreeWidth)).toBe(expected);
  });

  it.each([
    [NARROW_COMMIT_TREE_WIDTH_WHEN_COMPACT, 'wide'],
    [NARROW_COMMIT_TREE_WIDTH_WHEN_COMPACT - 1, 'narrow'],
    [VERY_NARROW_COMMIT_TREE_WIDTH_WHEN_COMPACT, 'narrow'],
    [VERY_NARROW_COMMIT_TREE_WIDTH_WHEN_COMPACT - 1, 'very-narrow'],
  ] as const)('uses compact breakpoints to categorize a width of %s as %s', (width, expected) => {
    writeAtom(renderCompactAtom, true);
    writeAtom(mainContentWidthState, width);
    expect(readAtom(commitTreeWidth)).toBe(expected);
  });
});
