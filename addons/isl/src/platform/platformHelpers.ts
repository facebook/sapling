/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {readAtom} from '../jotaiUtils';
import {repositoryInfo} from '../serverAPIState';
export function getRepoRoot() {
  const info = readAtom(repositoryInfo);
  if (info && info.type === 'success') {
    return info.repoRoot.replace(/\\/g, '/');
  } else {
    return undefined;
  }
}
