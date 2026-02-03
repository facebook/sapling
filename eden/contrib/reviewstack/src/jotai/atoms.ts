/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/**
 * This file contains Jotai atoms that are being migrated from Recoil.
 * See README.md in this directory for migration instructions.
 *
 * As atoms are migrated from recoil.ts, they should be added here.
 * Once all consumers of an atom are updated, the corresponding Recoil
 * atom can be removed from recoil.ts.
 */

import type {DiffAndTokenizeParams, DiffAndTokenizeResponse} from '../diffServiceWorker';
import type {DiffSide} from '../generated/graphql';
import type {DiffCommitIDs} from '../github/diffTypes';
import type {GitHubPullRequestReviewThread} from '../github/pullRequestTimelineTypes';
import type {Version} from '../github/types';
import type {LineToPosition} from '../lineToPosition';
import type {NewCommentInputCallbacks} from '../recoil';

import {atom} from 'jotai';
import {atomFamily} from 'jotai-family';
import {atomWithStorage, loadable} from 'jotai/utils';

/**
 * Migrated from: primerColorMode in themeState.ts
 *
 * See https://primer.style/react/theming#color-modes-and-color-schemes
 * Note that "day" is the default. Currently, we choose not to include "auto"
 * because <ThemeProvider> does not appear to support an event to tell us
 * when the colorMode changes.
 */
export type SupportedPrimerColorMode = 'day' | 'night';

const LOCAL_STORAGE_KEY = 'reviewstack-color-mode';

export const primerColorModeAtom = atomWithStorage<SupportedPrimerColorMode>(
  LOCAL_STORAGE_KEY,
  'day',
);

// =============================================================================
// Recoil Bridge Atoms
// =============================================================================
// These atoms bridge Recoil selectors to Jotai during the migration period.
// They allow components to use Jotai hooks while the underlying data still
// comes from Recoil. Once the full migration is complete, these can be
// replaced with native Jotai implementations.
//
// The bridge works by storing a "setter" function that Recoil components call
// to push updates into Jotai atoms. Components using Jotai will reactively
// update when the values change.
// =============================================================================

/**
 * Bridge atom for diffAndTokenize results.
 * Key is a serialized version of DiffAndTokenizeParams.
 */
export const diffAndTokenizeResultAtom = atomFamily(
  (_params: string) => atom<DiffAndTokenizeResponse | null>(null),
  (a, b) => a === b,
);

/**
 * Bridge atom for gitHubThreadsForDiffFile results.
 */
export const gitHubThreadsForDiffFileResultAtom = atomFamily(
  (_path: string) => atom<{[key in DiffSide]: GitHubPullRequestReviewThread[]} | null>(null),
  (a, b) => a === b,
);

/**
 * Bridge atom for gitHubDiffNewCommentInputCallbacks.
 */
export const gitHubDiffNewCommentInputCallbacksAtom = atom<NewCommentInputCallbacks | null>(null);

/**
 * Bridge atom for gitHubDiffCommitIDs.
 */
export const gitHubDiffCommitIDsAtom = atom<DiffCommitIDs | null>(null);

/**
 * Bridge atom for gitHubPullRequestVersions.
 */
export const gitHubPullRequestVersionsAtom = atom<Version[]>([]);

/**
 * Bridge atom for gitHubPullRequestSelectedVersionIndex.
 */
export const gitHubPullRequestSelectedVersionIndexAtom = atom<number>(0);

/**
 * Bridge atom for gitHubPullRequestLineToPositionForFile results.
 */
export const gitHubPullRequestLineToPositionForFileResultAtom = atomFamily(
  (_path: string) => atom<LineToPosition | null>(null),
  (a, b) => a === b,
);

/**
 * Combined atom that loads all the data needed for SplitDiffView.
 * This replaces the waitForAll([...]) pattern from Recoil.
 */
export type SplitDiffViewData = {
  diffAndTokenize: DiffAndTokenizeResponse;
  threads: {[key in DiffSide]: GitHubPullRequestReviewThread[]} | null;
  newCommentInputCallbacks: NewCommentInputCallbacks | null;
  commitIDs: DiffCommitIDs | null;
};

export const splitDiffViewDataAtom = atomFamily(
  (_params: {path: string; paramsKey: string; isPullRequest: boolean}) => {
    const baseAtom = atom<SplitDiffViewData | null>(null);
    return loadable(baseAtom);
  },
  (a, b) => a.path === b.path && a.paramsKey === b.paramsKey && a.isPullRequest === b.isPullRequest,
);

/**
 * Serializes DiffAndTokenizeParams for use as atomFamily keys.
 */
export function serializeDiffParams(params: DiffAndTokenizeParams): string {
  return JSON.stringify({
    path: params.path,
    before: params.before,
    after: params.after,
    scopeName: params.scopeName,
    colorMode: params.colorMode,
  });
}
