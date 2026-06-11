/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Platform} from '../platform';
import type {ThemeColor} from '../theme';
import type {Disposable} from '../types';

import {makeBrowserLikePlatformImpl} from './browserPlatformImpl';

// important: this file should not try to import other code from 'isl',
// since it will end up getting duplicated when bundling.

// The host (parent) window owns a tri-state light/dark/system theme setting and
// drives ISL's theme, passed in synchronously via the `?theme=` URL param.
type HostThemeSetting = 'light' | 'dark' | 'system';

function isHostThemeSetting(value: string | undefined): value is HostThemeSetting {
  return value === 'light' || value === 'dark' || value === 'system';
}

const darkMediaQuery =
  typeof window !== 'undefined' && typeof window.matchMedia === 'function'
    ? window.matchMedia('(prefers-color-scheme: dark)')
    : undefined;

function systemTheme(): ThemeColor {
  return darkMediaQuery?.matches ? 'dark' : 'light';
}

// agentHome just uses all the same defaults as the browser-like platform.
const base = makeBrowserLikePlatformImpl('agentHome');

// Seed the host setting from the `?theme=` URL param, defaulting to 'system'.
const paramTheme = base.initialUrlParams?.get('theme');
const hostSetting: HostThemeSetting = isHostThemeSetting(paramTheme) ? paramTheme : 'system';

// Resolve the host's tri-state setting into the concrete light/dark ISL needs.
function resolveTheme(): ThemeColor {
  return hostSetting === 'system' ? systemTheme() : hostSetting;
}

const agentHome: Platform = {
  ...base,
  theme: {
    getTheme: resolveTheme,
    getThemeName: () => hostSetting,
    // Only 'system' is reactive: follow OS light/dark changes. Explicit
    // light/dark from the host is fixed for the lifetime of the page.
    onDidChangeTheme(callback: (theme: ThemeColor) => unknown): Disposable {
      if (hostSetting !== 'system' || darkMediaQuery == null) {
        return {dispose: () => {}};
      }
      const handler = () => callback(resolveTheme());
      darkMediaQuery.addEventListener('change', handler);
      return {dispose: () => darkMediaQuery.removeEventListener('change', handler)};
    },
  },
};

window.islPlatform = agentHome;

// Load the actual app entry, which must be done after the platform has been set up.
import('../index');
