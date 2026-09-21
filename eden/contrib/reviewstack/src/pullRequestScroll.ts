/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

export type PullRequestScrollPosition = {
  scrollLeft: number;
  scrollTop: number;
  timelineScrollTop: number;
};

const DIFF_SCROLL_SELECTOR = '[data-reviewstack-diff-scroll="true"]';
const TIMELINE_SCROLL_SELECTOR = '[data-reviewstack-timeline-scroll="true"]';

export function capturePullRequestScrollPosition(): PullRequestScrollPosition {
  const diffContainer = document.querySelector<HTMLElement>(DIFF_SCROLL_SELECTOR);
  const timelineContainer = document.querySelector<HTMLElement>(TIMELINE_SCROLL_SELECTOR);
  return {
    scrollLeft: diffContainer?.scrollLeft ?? 0,
    scrollTop: diffContainer?.scrollTop ?? 0,
    timelineScrollTop: timelineContainer?.scrollTop ?? 0,
  };
}

export function restorePullRequestScrollPosition(position: PullRequestScrollPosition): void {
  const diffContainer = document.querySelector<HTMLElement>(DIFF_SCROLL_SELECTOR);
  if (diffContainer != null) {
    diffContainer.scrollLeft = position.scrollLeft;
    diffContainer.scrollTop = position.scrollTop;
  }
  const timelineContainer = document.querySelector<HTMLElement>(TIMELINE_SCROLL_SELECTOR);
  if (timelineContainer != null) {
    timelineContainer.scrollTop = position.timelineScrollTop;
  }
}
