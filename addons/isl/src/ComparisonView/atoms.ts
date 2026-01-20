/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Comparison} from 'shared/Comparison';

import {atom} from 'jotai';
import {ComparisonType} from 'shared/Comparison';
import {writeAtom} from '../jotaiUtils';
import platform from '../platform';

export type ComparisonMode = {
  comparison: Comparison;
  visible: boolean;
  /** Optional file path to scroll to when the comparison view opens */
  scrollToFile?: string;
};
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

/** Open Comparison View for a given comparison type, optionally scrolling to a specific file */
export async function showComparison(comparison: Comparison, scrollToFile?: string) {
  if (await platform.openDedicatedComparison?.(comparison)) {
    return;
  }
  writeAtom(currentComparisonMode, {comparison, visible: true, scrollToFile});
}

export function dismissComparison() {
  writeAtom(currentComparisonMode, last => ({...last, visible: false}));
}
