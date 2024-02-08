/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Comparison} from 'shared/Comparison';

import {atom} from 'jotai';
import {ComparisonType} from 'shared/Comparison';

export type ComparisonMode = {comparison: Comparison; visible: boolean};
export const currentComparisonMode = atom<ComparisonMode>({
  comparison: {type: ComparisonType.UncommittedChanges},
  visible: false,
});
