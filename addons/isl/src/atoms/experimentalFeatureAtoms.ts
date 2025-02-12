/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {atom} from 'jotai';
import {configBackedAtom, localStorageBackedAtom} from '../jotaiUtils';

// This config is intended to be controlled remotely. So it's read-only.
const remoteExperimentalFeatures = configBackedAtom<boolean | null>(
  'isl.experimental-features',
  false,
  true /* read-only */,
);

// 0: Respect remote config. 1: Enable experimental features. 2: Disable experimental features.
const localExperimentalFeatures = localStorageBackedAtom<number>(
  'isl.experimental-features-local-override',
  0,
);

/**
 * List of all currently enabled experimental features, as UI labels.
 * UI setting to enable experimental features is only shown if this list is non-empty.
 */
export const currentExperimentalFeaturesList: Array<string> = [];

/**
 * Whether experimental features are enabled.
 * Backed by a remote config by default. Can also be set locally.
 */
export const hasExperimentalFeatures = atom(
  get => {
    const localOverride = get(localExperimentalFeatures);
    if (localOverride === 1) {
      return true;
    } else if (localOverride === 2) {
      return false;
    } else {
      return get(remoteExperimentalFeatures) ?? false;
    }
  },
  (get, set, update) => {
    const newValue = typeof update === 'function' ? update(get(hasExperimentalFeatures)) : update;
    set(localExperimentalFeatures, newValue ? 1 : 2);
  },
);
