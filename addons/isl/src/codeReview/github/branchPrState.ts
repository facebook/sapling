/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {atom} from 'jotai';

/** Experimental backdoor setting to get around disabled submit modes. Useful for testing. */
export const overrideDisabledSubmitModes = atom(process.env.NODE_ENV === 'development');
