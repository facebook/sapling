/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Platform} from '../platform';

import {browserPlatformImpl} from './browerPlatformImpl';

// important: this file should not try to import other code from 'isl',
// since it will end up getting duplicated when bundling.

const androidStudioRemotePlatform: Platform = {
  ...browserPlatformImpl, // just act like the browser platform by default, since the remote use case is almost identical.
  platformName: 'androidStudioRemote',
  upsellExternalMergeTool: false,
};

window.islPlatform = androidStudioRemotePlatform;

// Load the actual app entry, which must be done after the platform has been set up.
import('../index');
