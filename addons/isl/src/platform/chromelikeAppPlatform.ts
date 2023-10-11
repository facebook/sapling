/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Platform} from '../platform';

import {browserPlatform} from '../BrowserPlatform';

// important: this file should not try to import other code from 'isl',
// since it will end up getting duplicated by webpack.

const chromelikeAppPlatform: Platform = {
  ...browserPlatform, // just act like the browser platform, since the chromelike app use case exactly identical.
  platformName: 'chromelike_app',
};

window.islPlatform = chromelikeAppPlatform;
