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

// combined state
// - read: nullable local theme -> platform theme -> 'dark'
// - write: update local theme
export const themeState = atom<ThemeColor, [ThemeColor], void>(
  get => get(localThemeState) ?? get(platformThemeState) ?? 'dark',
  (_get, set, themeColor) => set(localThemeState, themeColor),
);

/**
 * The specific theme name, like "Default Light Modern".
 * Typically, you'd rather use `themeState` to get simply "light" / "dark".
 * Theme name is useful for dynamically updating stylex styles for specific themes.
 */
export const themeNameState = atom<string | undefined>(theme?.getThemeName?.());

export function useThemeShortcut() {
  useCommand('ToggleTheme', () => {
    if (platform.theme == null) {
      writeAtom(localThemeState, theme => (theme === 'dark' ? 'light' : 'dark'));
    }
  });
}
