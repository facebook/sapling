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
  ])(
    'preserves theme and reload URL from %s without a browser fallback',
    async (query, expected) => {
      window.history.replaceState({}, '', `/obsidian.html${query}`);
      await jest.isolateModulesAsync(async () => {
        await import('../obsidianPlatform');

        const {getBrowserPlatform} = await import('../../BrowserPlatform');

        expect(getBrowserPlatform()).toBe(window.islPlatform);
        expect(localStorage.getItem('ISLInitialParams')).toBeNull();
        expect({
          query: window.location.search,
          theme: window.islPlatform?.theme?.getTheme(),
        }).toEqual({query, theme: expected});
      });
    },
  );
});
