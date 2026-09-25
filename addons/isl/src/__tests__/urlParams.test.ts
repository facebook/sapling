/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {logger} from '../logger';
import {computeInitialParams} from '../urlParams';

const storageKey = 'ISLInitialParams';
const token = 'test-launch-capability';

describe('initial URL credentials', () => {
  beforeEach(() => {
    localStorage.clear();
    window.history.replaceState({}, '', '/');
  });

  afterEach(() => {
    jest.restoreAllMocks();
    localStorage.clear();
    window.history.replaceState({}, '', '/');
    delete window.islPlatform;
  });

  it.each([false, true])('does not log URL values (browser: %s)', isBrowser => {
    window.history.replaceState({}, '', `/?token=${token}&cwd=%2Frepo`);

    expect(computeInitialParams(isBrowser).get('token')).toBe(token);

    expect(logger.log).toHaveBeenCalledWith('Loaded initial params from URL');
    expect(jest.mocked(logger.log).mock.calls.every(args => args.length === 1)).toBe(true);
    expect(window.location.search).toBe(isBrowser ? '' : `?token=${token}&cwd=%2Frepo`);
    expect(localStorage.getItem(storageKey)).toBe(
      isBrowser
        ? JSON.stringify([
            ['token', token],
            ['cwd', '/repo'],
          ])
        : null,
    );
  });

  it('restores browser reload credentials without logging them', () => {
    localStorage.setItem(storageKey, JSON.stringify([['token', token]]));

    expect(computeInitialParams(true).get('token')).toBe(token);

    expect(logger.log).toHaveBeenCalledWith('Loaded initial params from local storage');
    expect(jest.mocked(logger.log).mock.calls.every(args => args.length === 1)).toBe(true);
  });

  it('does not log malformed cached credentials', () => {
    localStorage.setItem(storageKey, `invalid ${token}`);

    expect(computeInitialParams(true).size).toBe(0);

    expect(logger.log).toHaveBeenCalledWith('Failed to load initial params from local storage');
  });

  it('does not load browser credentials for an embedding', () => {
    localStorage.setItem(storageKey, JSON.stringify([['token', token]]));

    expect(computeInitialParams(false).size).toBe(0);
  });

  it.each([false, true])('retains launch parameters on reload (browser: %s)', isBrowser => {
    window.history.replaceState({}, '', `/?token=${token}&cwd=%2Frepo&theme=dark`);

    const firstLoad = computeInitialParams(isBrowser);
    const reload = computeInitialParams(isBrowser);

    expect(reload).toEqual(firstLoad);
    expect(reload.get('token')).toBe(token);
    expect(reload.get('cwd')).toBe('/repo');
    expect(reload.get('theme')).toBe('dark');
    if (!isBrowser) {
      expect(localStorage.getItem(storageKey)).toBeNull();
    }
  });

  it('omits credential-bearing storage failures from diagnostics', () => {
    window.history.replaceState({}, '', `/?token=${token}`);
    jest.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error(`failed to store ${token}`);
    });

    expect(computeInitialParams(true).get('token')).toBe(token);

    expect(logger.log).toHaveBeenCalledWith('Failed to save initial params to local storage');
    expect(jest.mocked(logger.log).mock.calls.every(args => args.length === 1)).toBe(true);
  });

  it('does not construct the browser fallback when an embedding is installed', async () => {
    window.history.replaceState({}, '', `/?token=${token}`);
    await jest.isolateModulesAsync(async () => {
      const {makeBrowserLikePlatformImpl} = await import('../platform/browserPlatformImpl');

      expect(window.location.search).toBe(`?token=${token}`);
      expect(localStorage.getItem(storageKey)).toBeNull();

      const embedded = makeBrowserLikePlatformImpl('agentHome');
      window.islPlatform = embedded;

      const {default: platform} = await import('../platform');

      expect(platform).toBe(embedded);
      expect(embedded.initialUrlParams?.get('token')).toBe(token);
      expect(window.location.search).toBe(`?token=${token}`);
      expect(localStorage.getItem(storageKey)).toBeNull();
    });
  });

  it('constructs a standalone browser platform when no embedding is installed', async () => {
    window.history.replaceState({}, '', `/?token=${token}`);
    await jest.isolateModulesAsync(async () => {
      const {default: platform} = await import('../platform');

      expect(platform.platformName).toBe('browser');
      expect(platform.initialUrlParams?.get('token')).toBe(token);
      expect(window.islPlatform).toBe(platform);
      expect(window.location.search).toBe('');
    });
  });
});
