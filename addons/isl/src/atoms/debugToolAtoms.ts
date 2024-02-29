/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {localStorageBackedAtom} from '../jotaiUtils';

export const enableReduxTools = localStorageBackedAtom<boolean>('isl.debug-redux-tools', false);

export const enableReactTools = localStorageBackedAtom<boolean>('isl.debug-react-tools', false);
