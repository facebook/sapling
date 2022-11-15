/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import App from './App';
import {setCustomLinkElement} from './Link';
import {getColorModeFromLocalStorage} from './themeState';
import {setCustomNavigateHook} from './useNavigate';
import {ThemeProvider} from '@primer/react';
export {
  App,
  getColorModeFromLocalStorage,
  setCustomLinkElement,
  setCustomNavigateHook,
  ThemeProvider,
};
