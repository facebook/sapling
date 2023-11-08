/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hash} from '../types';

import {latestCommits} from '../serverAPIState';
import {allDiffSummaries, codeReviewProvider} from './CodeReviewInfo';
import {selector} from 'recoil';

export enum SyncStatus {
  InSync = 'inSync',
  LocalIsNewer = 'localIsNewer',
  RemoteIsNewer = 'remoteIsNewer',
}

export const syncStatusAtom = selector<undefined | Map<Hash, SyncStatus>>({
  key: 'syncStatusAtom',
  get: ({get}) => {
    const provider = get(codeReviewProvider);
    if (provider == null) {
      return undefined;
    }
    const commits = get(latestCommits);
    const summaries = get(allDiffSummaries);
    if (summaries.value == null) {
      return undefined;
    }
    return provider.getSyncStatuses(commits, summaries.value);
  },
});
