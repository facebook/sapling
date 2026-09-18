/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

export type PullRequestScrollPosition = {
  scrollLeft: number;
  scrollTop: number;
};

const DIFF_SCROLL_SELECTOR = '[data-reviewstack-diff-scroll="true"]';

export function capturePullRequestScrollPosition(): PullRequestScrollPosition {
  const container = document.querySelector<HTMLElement>(DIFF_SCROLL_SELECTOR);
  return {
    scrollLeft: container?.scrollLeft ?? 0,
    scrollTop: container?.scrollTop ?? 0,
  };
}

export function restorePullRequestScrollPosition(position: PullRequestScrollPosition): void {
  const container = document.querySelector<HTMLElement>(DIFF_SCROLL_SELECTOR);
  if (container != null) {
    container.scrollLeft = position.scrollLeft;
    container.scrollTop = position.scrollTop;
  }
}
