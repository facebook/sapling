/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// Note: we intentionally don't persist focus mode, so each time you open ISL,

import {atom} from 'jotai';

// you see all your commits and can choose to focus from that point onward.
export const focusMode = atom(false);
