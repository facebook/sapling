/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hash} from '../types';

import {atom} from 'jotai';
import {latestCommits} from '../serverAPIState';
import {allDiffSummaries, codeReviewProvider} from './CodeReviewInfo';

export enum SyncStatus {
  InSync = 'inSync',
  LocalIsNewer = 'localIsNewer',
  RemoteIsNewer = 'remoteIsNewer',
  BothChanged = 'bothChanged',
}

const emptyMap = new Map<Hash, SyncStatus>();

export const syncStatusAtom = atom(get => {
  const provider = get(codeReviewProvider);
  if (provider == null) {
    return emptyMap;
  }
  const commits = get(latestCommits);
  const summaries = get(allDiffSummaries);
  if (summaries.value == null) {
    return emptyMap;
  }
  return provider.getSyncStatuses(commits, summaries.value);
});
