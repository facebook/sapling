/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {configBackedAtom} from './jotaiUtils';
import {atom} from 'jotai';

// TODO: should we read `merge-tool.$tool` to check `.disabled`?
const uiMergeConfig = configBackedAtom<string | null>(
  'ui.merge',
  null,
  true,
  /* use raw value */ true,
);
export const externalMergeToolAtom = atom(get => {
  const config = get(uiMergeConfig);
  // filter out internal merge tools
  if (config == null || config.startsWith('internal:')) {
    return null;
  }
  return config;
});
