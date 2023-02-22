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

const androidStudioRemotePlatform: Platform = {
  ...browserPlatform, // just act like the browser platform by default, since the remote use case is almost identical.
  platformName: 'androidStudioRemote',
};

window.islPlatform = androidStudioRemotePlatform;
