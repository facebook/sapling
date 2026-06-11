/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {atom} from 'jotai';
import {useCommand} from './ISLShortcuts';
import {localStorageBackedAtom, writeAtom} from './jotaiUtils';
import platform from './platform';
import {registerDisposable} from './utils';

import 'isl-components/theme/themeDark.css';
import 'isl-components/theme/themeLight.css';

const THEME_LOCAL_STORAGE_KEY = 'isl-color-theme';

export type ThemeColor = 'dark' | 'light';

/**
 * User-facing theme preference shown in the theme picker.
 * - 'light' / 'dark': an explicit override
 * - 'system': follow the platform / system theme (no local override)
 */
export type ThemePreference = ThemeColor | 'system';

// local override. `null` means prefer platform theme.
const localThemeState = localStorageBackedAtom<ThemeColor | null>(THEME_LOCAL_STORAGE_KEY, null);

// platform theme. `null` means not supported.
const theme = platform.theme;
const platformThemeState = atom<ThemeColor | undefined>(theme?.getTheme());
registerDisposable(
  platform,
  theme?.onDidChangeTheme(themeColor => {
    writeAtom(platformThemeState, themeColor);
    // reset local theme state so the user can notice the theme change
    writeAtom(localThemeState, null);
    theme.getThemeName && writeAtom(themeNameState, theme.getThemeName());
  }) ?? {dispose: () => null},
  import.meta.hot,
);

/**
 * The standard, cross-browser `prefers-color-scheme` media query, or `undefined`
 * when `matchMedia` is unavailable (e.g. non-DOM environments).
 */
const systemThemeMediaQuery =
  typeof window !== 'undefined' && typeof window.matchMedia === 'function'
    ? window.matchMedia('(prefers-color-scheme: dark)')
    : undefined;

/** Read the system theme from `prefers-color-scheme`. */
function getSystemTheme(): ThemeColor | undefined {
  if (systemThemeMediaQuery == null) {
    return undefined;
  }
  return systemThemeMediaQuery.matches ? 'dark' : 'light';
}

// System theme derived from `prefers-color-scheme`, kept reactive via a 'change'
// listener so 'system' mode updates when the OS theme changes. This is used on
// platforms without their own `platform.theme` provider (platforms that do
// provide a theme handle their own reactivity above).
const systemThemeState = atom<ThemeColor | undefined>(getSystemTheme());
if (systemThemeMediaQuery != null) {
  const handler = () => writeAtom(systemThemeState, getSystemTheme());
  systemThemeMediaQuery.addEventListener('change', handler);
  registerDisposable(
    systemThemeMediaQuery,
    {dispose: () => systemThemeMediaQuery.removeEventListener('change', handler)},
    import.meta.hot,
  );
}

// combined state
// - read: nullable local theme -> platform theme -> system theme -> 'dark'.
//   In 'system' mode (no local override) without a platform theme provider, the
//   effective theme is derived from the OS/browser `prefers-color-scheme` and
//   updates reactively when it changes.
// - write: update local theme
export const themeState = atom<ThemeColor, [ThemeColor], void>(
  get => get(localThemeState) ?? get(platformThemeState) ?? get(systemThemeState) ?? 'dark',
  (_get, set, themeColor) => set(localThemeState, themeColor),
);

/**
 * Three-way theme preference backing the theme picker.
 * - read: maps the absence of a local override to 'system'
 * - write: 'system' clears the local override (falling back to the platform
 *   theme); 'light'/'dark' set an explicit override.
 */
export const themePreferenceState = atom<ThemePreference, [ThemePreference], void>(
  get => get(localThemeState) ?? 'system',
  (_get, set, preference) => set(localThemeState, preference === 'system' ? null : preference),
);

/**
 * The specific theme name, like "Default Light Modern".
 * Typically, you'd rather use `themeState` to get simply "light" / "dark".
 */
export const themeNameState = atom<string | undefined>(theme?.getThemeName?.());

export function useThemeShortcut() {
  useCommand('ToggleTheme', () => {
    if (platform.theme == null) {
      writeAtom(localThemeState, theme => (theme === 'dark' ? 'light' : 'dark'));
    }
  });
}
