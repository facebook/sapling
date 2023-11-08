/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Platform} from '../platform';

import {browserPlatformImpl} from './browerPlatformImpl';

// important: this file should not try to import other code from 'isl',
// since it will end up getting duplicated by webpack.

const chromelikeAppPlatform: Platform = {
  ...browserPlatformImpl,

  platformName: 'chromelike_app',
};

window.islPlatform = chromelikeAppPlatform;
