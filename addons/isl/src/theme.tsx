/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';

import platform from './platform';
import {atom, useRecoilValue} from 'recoil';

import './themeLight.css';
import './themeDark.css';

const THEME_LOCAL_STORAGE_KEY = 'isl-color-theme';

export type ThemeColor = 'dark' | 'light';
export const themeState = atom<ThemeColor>({
  key: 'themeState',
  default:
    platform.theme?.getTheme() ??
    (localStorage.getItem(THEME_LOCAL_STORAGE_KEY) as ThemeColor) ??
    'dark',
  effects: [
    // Persist changes to theme to local storage
    ({onSet}) => {
      onSet(newValue => {
        localStorage.setItem(THEME_LOCAL_STORAGE_KEY, newValue);
      });
    },
    ({setSelf}) => {
      const disposable = platform.theme?.onDidChangeTheme(theme => {
        setSelf(theme);
      });
      return () => disposable?.dispose();
    },
  ],
});

export function ThemeRoot({children}: {children: ReactNode}) {
  const theme = useRecoilValue(themeState);
  return <div className={`isl-root ${theme}-theme`}>{children}</div>;
}
