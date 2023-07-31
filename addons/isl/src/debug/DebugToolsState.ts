/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {atom} from 'recoil';

export const debugToolsEnabledState = atom<boolean>({
  key: 'debugToolsEnabledState',
  default: process.env.NODE_ENV === 'development',
});
