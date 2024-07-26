/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Comparison} from 'shared/Comparison';

import {writeAtom} from '../jotaiUtils';
import {atom} from 'jotai';
import {ComparisonType} from 'shared/Comparison';

export type ComparisonMode = {comparison: Comparison; visible: boolean};
export const currentComparisonMode = atom<ComparisonMode>(
  window.islAppMode?.mode === 'comparison'
    ? {
        comparison: window.islAppMode.comparison,
        visible: true,
      }
    : {
        comparison: {type: ComparisonType.UncommittedChanges},
        visible: false,
      },
);

/** Open Comparison View for a given comparison type */
export function showComparison(comparison: Comparison) {
  writeAtom(currentComparisonMode, {comparison, visible: true});
}

export function dismissComparison() {
  writeAtom(currentComparisonMode, last => ({...last, visible: false}));
}
