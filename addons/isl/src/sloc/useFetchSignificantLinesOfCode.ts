/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo} from '../types';

import serverAPI from '../ClientToServerAPI';
import {commitInfoViewCurrentCommits} from '../CommitInfoView/CommitInfoState';
import {getGeneratedFilesFrom} from '../GeneratedFile';
import {atomFamilyWeak, lazyAtom} from '../jotaiUtils';
import {isFullyOrPartiallySelected} from '../partialSelection';
import {uncommittedChangesWithPreviews} from '../previews';
import {GeneratedStatus} from '../types';
import {MAX_FILES_ALLOWED_FOR_DIFF_STAT} from './diffStatConstants';
import {atom, useAtom, useAtomValue} from 'jotai';
import {loadable} from 'jotai/utils';
import {useRef} from 'react';

const commitSloc = atomFamilyWeak((hash: string) => {
  return lazyAtom(async get => {
    const commits = get(commitInfoViewCurrentCommits);
    if (commits == null || commits.length > 1) {
      return undefined;
    }
    const [commit] = commits;
    if (commit.totalFileCount > MAX_FILES_ALLOWED_FOR_DIFF_STAT) {
      return undefined;
    }
    const filesToQueryGeneratedStatus = commit.filesSample.map(f => f.path);
    const generatedStatuses = getGeneratedFilesFrom(filesToQueryGeneratedStatus);

    const excludedFiles = filesToQueryGeneratedStatus.reduce<string[]>((filtered, path) => {
      // the __generated__ pattern is included in the exclusions, so we don't need to include it here
      if (!path.match(/__generated__/) && generatedStatuses[path] !== GeneratedStatus.Manual) {
        filtered.push(path);
      }
      return filtered;
    }, []);
    serverAPI.postMessage({
      type: 'fetchSignificantLinesOfCode',
      hash,
      excludedFiles,
    });

    const loc = await serverAPI
      .nextMessageMatching('fetchedSignificantLinesOfCode', message => message.hash === hash)
      .then(result => result.linesOfCode);

    return loc.value;
  }, undefined);
});

let requestId = 0;
const pendingChangesSlocAtom = atom(async get => {
  // this atom makes use of the fact that jotai will only use the most recently created request (ignoring older requests)
  // to avoid race conditions when the response from an older request is sent after a newer one
  // so for example:
  // requestId A (slow) => Server (sleeps 5 sec)
  // requestId B (fast) => Server responds immediately, client updates
  // requestId A (slow) => Server responds, client ignores
  const commits = get(commitInfoViewCurrentCommits);
  if (commits == null || commits.length > 1) {
    return undefined;
  }
  const [commit] = commits;
  if (commit.totalFileCount > MAX_FILES_ALLOWED_FOR_DIFF_STAT) {
    return undefined;
  }
  const isPathFullorPartiallySelected = get(isFullyOrPartiallySelected);

  const uncommittedChanges = get(uncommittedChangesWithPreviews);
  const selectedFiles = uncommittedChanges.reduce((selected, f) => {
    if (!f.path.match(/__generated__/) && isPathFullorPartiallySelected(f.path)) {
      selected.push(f.path);
    }
    return selected;
  }, [] as string[]);

  if (selectedFiles.length > MAX_FILES_ALLOWED_FOR_DIFF_STAT) {
    return undefined;
  }

  if (selectedFiles.length === 0) {
    return 0;
  }
  requestId += 1;
  serverAPI.postMessage({
    type: 'fetchPendingSignificantLinesOfCode',
    hash: commit.hash,
    includedFiles: selectedFiles,
    requestId,
  });

  const pendingLoc = await serverAPI
    .nextMessageMatching(
      'fetchedPendingSignificantLinesOfCode',
      message => message.requestId == requestId && message.hash === commit.hash,
    )
    .then(result => result.linesOfCode);
  return pendingLoc.value;
});
const pendingChangesSloc = loadable(pendingChangesSlocAtom);

export function useFetchSignificantLinesOfCode(commit: CommitInfo) {
  return useAtomValue(commitSloc(commit.hash));
}

export function useFetchPendingSignificantLinesOfCode() {
  const previous = useRef<number | undefined>(undefined);
  const pendingChanges = useAtomValue(pendingChangesSloc);
  if (pendingChanges.state === 'hasError') {
    throw pendingChanges.error;
  }
  if (pendingChanges.state === 'loading') {
    //using the previous value in the loading state to avoid flickering / jankiness in the UI
    return previous.current;
  }

  previous.current = pendingChanges.data;

  return pendingChanges.data;
}
