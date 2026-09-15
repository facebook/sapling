/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

jest.mock('../../index', () => ({}));

describe('Obsidian platform theme', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  afterEach(() => {
    delete window.islPlatform;
    window.history.replaceState({}, '', '/');
    localStorage.clear();
  });

  it.each([
    ['?theme=light', 'light'],
    ['?theme=dark', 'dark'],
    ['', 'dark'],
  ])('preserves the initial theme from %s after URL cleanup', async (query, expected) => {
    window.history.replaceState({}, '', `/obsidian.html${query}`);
    await jest.isolateModulesAsync(async () => {
      await import('../obsidianPlatform');

      // The app imports this fallback even when window.islPlatform is already set.
      await import('../../BrowserPlatform');

      expect({
        query: window.location.search,
        theme: window.islPlatform?.theme?.getTheme(),
      }).toEqual({query: '', theme: expected});
    });
  });
});
