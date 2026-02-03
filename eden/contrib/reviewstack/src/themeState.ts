/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/**
 * See https://primer.style/react/theming#color-modes-and-color-schemes
 * Note that "day" is the default. Currently, we choose not to include "auto"
 * because <ThemeProvider> does not appear to support an event to tell us
 * when the colorMode changes?
 *
 * @deprecated Use SupportedPrimerColorMode from './jotai/atoms' instead.
 * This type is kept for backwards compatibility with index.html.
 */
export type SupportedPrimerColorMode = 'day' | 'night';

const LOCAL_STORAGE_KEY = 'reviewstack-color-mode';

/**
 * Gets the color mode from localStorage for initial page render.
 * Used by reviewstack.dev/public/index.html to set initial theme.
 */
export function getColorModeFromLocalStorage(): SupportedPrimerColorMode {
  return localStorage.getItem(LOCAL_STORAGE_KEY) !== 'night' ? 'day' : 'night';
}
