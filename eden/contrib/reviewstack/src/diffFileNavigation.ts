/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DisplayChange} from './coalesceRenamedFiles';

import joinPath from './joinPath';

export function getDisplayChangePath(change: DisplayChange): string {
  switch (change.type) {
    case 'add':
    case 'remove':
      return joinPath(change.basePath, change.entry.name);
    case 'modify':
      return joinPath(change.basePath, change.after.name);
    case 'rename':
      return joinPath(change.after.basePath, change.after.entry.name);
  }
}

export function getDisplayChangeLabel(change: DisplayChange): string {
  if (change.type !== 'rename') {
    return getDisplayChangePath(change);
  }
  const previousPath = joinPath(change.before.basePath, change.before.entry.name);
  return `${previousPath} → ${getDisplayChangePath(change)}`;
}

export function diffFileAnchorID(path: string): string {
  return `reviewstack-diff-file-${encodeURIComponent(path)}`;
}

export function scrollToDiffFile(path: string): void {
  const target = document.getElementById(diffFileAnchorID(path));
  const diffContainer = target?.parentElement;
  if (target == null || diffContainer == null) {
    return;
  }

  const scrollToTarget = () => target.scrollIntoView({block: 'start'});
  scrollToTarget();

  // Individual file diffs load independently. Keep the selected file anchored
  // while earlier files expand from placeholders to their full height.
  let settleTimer: number | undefined;
  const observer = new ResizeObserver(() => {
    scrollToTarget();
    window.clearTimeout(settleTimer);
    settleTimer = window.setTimeout(() => observer.disconnect(), 300);
  });
  observer.observe(diffContainer);
  window.setTimeout(() => observer.disconnect(), 5000);
}
