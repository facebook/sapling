/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {PageVisibility} from './types';

const ACTIVITY_RANK: Record<PageVisibility, number> = {
  hidden: 0,
  visible: 1,
  focused: 2,
};

export function combinePageVisibility(
  browserVisibility: PageVisibility,
  platformVisibility?: PageVisibility,
): PageVisibility {
  if (platformVisibility == null) {
    return browserVisibility;
  }
  return ACTIVITY_RANK[browserVisibility] <= ACTIVITY_RANK[platformVisibility]
    ? browserVisibility
    : platformVisibility;
}

export function browserPageVisibility(doc: Document): PageVisibility {
  return doc.hasFocus() ? 'focused' : doc.visibilityState;
}

export function isPageVisibility(value: unknown): value is PageVisibility {
  return value === 'focused' || value === 'visible' || value === 'hidden';
}
