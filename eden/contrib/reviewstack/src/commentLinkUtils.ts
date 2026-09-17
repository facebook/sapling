/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ID} from './github/types';

export type CommentLocation = 'timeline' | 'diff';

export function commentAnchorID(id: ID, location: CommentLocation = 'timeline'): string {
  return `${location}-comment-${id}`;
}

export function commentPermalink(
  id: ID,
  location: CommentLocation = 'timeline',
  currentURL: string = window.location.href,
): string {
  const url = new URL(currentURL);
  url.hash = commentAnchorID(id, location);
  return url.href;
}
