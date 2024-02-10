/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hash} from '../types';

import {latestCommitsJotai} from '../serverAPIState';
import {allDiffSummaries, codeReviewProviderJotai} from './CodeReviewInfo';
import {atom} from 'jotai';
import {atomFamily} from 'jotai/utils';

export enum SyncStatus {
  InSync = 'inSync',
  LocalIsNewer = 'localIsNewer',
  RemoteIsNewer = 'remoteIsNewer',
}

export const syncStatusAtom = atom(get => {
  const provider = get(codeReviewProviderJotai);
  if (provider == null) {
    return undefined;
  }
  const commits = get(latestCommitsJotai);
  const summaries = get(allDiffSummaries);
  if (summaries.value == null) {
    return undefined;
  }
  return provider.getSyncStatuses(commits, summaries.value);
});

export const syncStatusByHash = atomFamily((hash: Hash) =>
  atom(get => {
    const syncStatus = get(syncStatusAtom);
    return syncStatus?.get(hash);
  }),
);
