/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DiffComment} from 'isl/src/types';

export type ClientToServerMessage = {type: 'squareIt'; value: number} | {type: 'fetchDiffComment'};

export type ServerToClientMessage =
  | {type: 'gotSquared'; result: number}
  | {type: 'fetchedDiffComment'; hash: string; comment: DiffComment};
