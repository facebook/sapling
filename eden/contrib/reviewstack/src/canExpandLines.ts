/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {GitObjectID} from './github/types';

/** A collapsed range needs both original content and at least one hidden line. */
export default function canExpandLines(
  numLines: number,
  beforeOID: GitObjectID | null,
): beforeOID is GitObjectID {
  return numLines > 0 && beforeOID != null;
}
