/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {combinePageVisibility, isPageVisibility} from '../platformVisibility';

describe('platform visibility', () => {
  it.each([
    ['focused', undefined, 'focused'],
    ['visible', undefined, 'visible'],
    ['focused', 'visible', 'visible'],
    ['visible', 'focused', 'visible'],
    ['hidden', 'focused', 'hidden'],
    ['focused', 'hidden', 'hidden'],
  ] as const)('combines browser %s and platform %s as %s', (browser, parent, expected) => {
    expect(combinePageVisibility(browser, parent)).toBe(expected);
  });

  it('accepts only the three visibility states', () => {
    expect(isPageVisibility('focused')).toBe(true);
    expect(isPageVisibility('visible')).toBe(true);
    expect(isPageVisibility('hidden')).toBe(true);
    expect(isPageVisibility('background')).toBe(false);
    expect(isPageVisibility(null)).toBe(false);
  });
});
