/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Loadable} from 'recoil';

import {colorMap} from './diffServiceClient';
import {updateTextMateGrammarCSS} from './textmate/textmateStyles';
import {atom, noWait, selector} from 'recoil';

/**
 * See https://primer.style/react/theming#color-modes-and-color-schemes
 * Note that "day" is the default. Currently, we choose not to include "auto"
 * because <ThemeProvider> does not appear to support an event to tell us
 * when the colorMode changes?
 */
export type SupportedPrimerColorMode = 'day' | 'night';

const LOCAL_STORAGE_KEY = 'reviewstack-color-mode';

export function getColorModeFromLocalStorage(): SupportedPrimerColorMode {
  return localStorage.getItem(LOCAL_STORAGE_KEY) !== 'night' ? 'day' : 'night';
}

export const primerColorMode = atom<SupportedPrimerColorMode>({
  key: 'primerColorMode',
  // We define the default value as a selector to give us an opportunity to call
  // updateTextMateGrammarCSS() with the initial value of this atom.
  default: selector({
    key: 'primerColorMode/default',
    get: ({get}) => {
      const colorMode = getColorModeFromLocalStorage();
      const loadable = get(noWait(colorMap(colorMode)));
      scheduleCSSUpdate(colorMode, loadable);
      return colorMode;
    },
  }),
  effects: [
    // Persist the user's preference in localStorage.
    ({onSet}) => {
      onSet(newValue => {
        localStorage.setItem(LOCAL_STORAGE_KEY, newValue);
      });
    },
    // Update the global <style> element for TextMate CSS to reflect the change
    // to the page's theme.
    ({onSet, getLoadable}) => {
      onSet(colorMode => {
        const loadable = getLoadable(colorMap(colorMode));
        scheduleCSSUpdate(colorMode, loadable);
      });
    },
  ],
});

function scheduleCSSUpdate(colorMode: SupportedPrimerColorMode, loadable: Loadable<string[]>) {
  const colorScheme = colorMode === 'day' ? 'light' : 'dark';
  // Note that we update the colorScheme on document.documentElement rather than
  // <App>, but we can revisit this if it is an issue.
  document.documentElement.style.colorScheme = colorScheme;

  // We try to use getLoadable().valueMaybe() rather than getPromise() to
  // try to minimize the delay between the user toggling the theme switch
  // and seeing the update to syntax highlighting.
  const map = loadable.valueMaybe();
  if (map != null) {
    updateTextMateGrammarCSS(map);
  } else {
    loadable.toPromise().then(updateTextMateGrammarCSS);
  }
}
