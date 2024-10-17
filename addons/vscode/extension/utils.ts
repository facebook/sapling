/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import packageJson from '../package.json';

/** The version of the vscode extension, defined by the package.json (at build time),
 * or '(dev)' when running from source in a dev build (from `yarn watch-extension`). */
export const extensionVersion =
  process.env.NODE_ENV === 'development' ? '(dev)' : packageJson.version;
