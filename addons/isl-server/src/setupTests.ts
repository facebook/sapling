/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Internal} from './Internal';

// ensure we never try to actually log analytics during tests
Internal.mockAnalytics?.();

export {};
