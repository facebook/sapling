/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo, SlocInfo} from '../types';
import type {Atom, Getter} from 'jotai';
import type {Loadable} from 'jotai/vanilla/utils/loadable';

import serverAPI from '../ClientToServerAPI';
import {commitInfoViewCurrentCommits} from '../CommitInfoView/CommitInfoState';
import {getGeneratedFilesFrom} from '../GeneratedFile';
import {pageVisibility} from '../codeReview/CodeReviewInfo';
import {atomFamilyWeak, lazyAtom} from '../jotaiUtils';
import {isFullyOrPartiallySelected} from '../partialSelection';
import {uncommittedChangesWithPreviews} from '../previews';
import {GeneratedStatus} from '../types';
import {MAX_FILES_ALLOWED_FOR_DIFF_STAT} from './diffStatConstants';
import {atom, useAtomValue} from 'jotai';
import {loadable} from 'jotai/utils';
import {useRef} from 'react';

const getGeneratedFiles = (files: string[]): string[] => {
  const generatedStatuses = getGeneratedFilesFrom(files);

  return files.reduce<string[]>((filtered, path) => {
    // check if the file should be excluded
    // the __generated__ pattern is included in the exclusions, so we don't need to include it here
    if (path.match(/__generated__/) && generatedStatuses[path] === GeneratedStatus.Manual) {
      filtered.push(path);
    }

    return filtered;
  }, []);
};

async function fetchSignificantLinesOfCode(
  commit: Readonly<CommitInfo>,
  additionalFilesToExclude: Readonly<string[]> = [],
  getExcludedFiles: (files: string[]) => string[] = getGeneratedFiles,
): Promise<SlocInfo> {
  const filesToQueryGeneratedStatus = commit.filesSample.map(f => f.path);
  const excludedFiles = getExcludedFiles(filesToQueryGeneratedStatus);

  serverAPI.postMessage({
    type: 'fetchSignificantLinesOfCode',
    hash: commit.hash,
    excludedFiles: [...excludedFiles, ...additionalFilesToExclude],
  });

  const slocData = await serverAPI
    .nextMessageMatching('fetchedSignificantLinesOfCode', message => message.hash === commit.hash)
    .then(result => ({
      sloc: result.result.value?.linesOfCode,
      strictSloc: result.result.value?.strictLinesOfCode,
    }));

  return slocData;
}

const commitSloc = atomFamilyWeak((_hash: string) => {
  return lazyAtom(async get => {
    const commits = get(commitInfoViewCurrentCommits);
    if (commits == null || commits.length > 1) {
      return undefined;
    }
    const [commit] = commits;
    if (commit.totalFileCount > MAX_FILES_ALLOWED_FOR_DIFF_STAT) {
      return undefined;
    }
    const sloc = await fetchSignificantLinesOfCode(commit);
    return sloc;
  }, undefined);
});

const selectedFilesAtom = atom(get => {
  const isPathFullorPartiallySelected = get(isFullyOrPartiallySelected);

  const uncommittedChanges = get(uncommittedChangesWithPreviews);
  const selectedFiles = uncommittedChanges.reduce((selected, f) => {
    if (!f.path.match(/__generated__/) && isPathFullorPartiallySelected(f.path)) {
      selected.push(f.path);
    }
    return selected;
  }, [] as string[]);

  return selectedFiles;
});

/**
 * FETCH PENDING AMEND SLOC
 */
const fetchPendingAmendSloc = async (
  get: Getter,
  includedFiles: string[],
  requestId: number,
): Promise<SlocInfo | undefined> => {
  const commits = get(commitInfoViewCurrentCommits);
  if (commits == null || commits.length > 1) {
    return undefined;
  }
  const [commit] = commits;
  if (commit.totalFileCount > MAX_FILES_ALLOWED_FOR_DIFF_STAT) {
    return undefined;
  }

  if (includedFiles.length > MAX_FILES_ALLOWED_FOR_DIFF_STAT) {
    return undefined;
  }

  if (includedFiles.length === 0) {
    return {sloc: 0, strictSloc: 0};
  }

  //the calculation here is a bit tricky but in nutshell it is:
  //   SLOC for unselected committed files
  // + SLOC for selected files (to be amended) in the commit
  // ---------------------------------------------------------------------------------------------------
  // => What SLOC would be after you do the amend.
  // this way we won't show the split suggestions when the net effect of the amend will actually reduce SLOC (reverting for example)

  //pass in the selected files to be excluded.
  const unselectedCommittedSlocInfo = await fetchSignificantLinesOfCode(commit, includedFiles);

  serverAPI.postMessage({
    type: 'fetchPendingAmendSignificantLinesOfCode',
    hash: commit.hash,
    includedFiles,
    requestId,
  });

  const pendingLoc = await serverAPI
    .nextMessageMatching(
      'fetchedPendingAmendSignificantLinesOfCode',
      message => message.requestId === requestId && message.hash === commit.hash,
    )
    .then(result => ({
      sloc: result.result.value?.linesOfCode,
      strictSloc: result.result.value?.strictLinesOfCode,
    }));

  if (unselectedCommittedSlocInfo === undefined) {
    return pendingLoc;
  }

  if (pendingLoc === undefined) {
    return unselectedCommittedSlocInfo;
  }

  const slocInfo = {
    sloc: (unselectedCommittedSlocInfo.sloc ?? 0) + (pendingLoc.sloc ?? 0),
    strictSloc: (unselectedCommittedSlocInfo.strictSloc ?? 0) + (pendingLoc.strictSloc ?? 0),
  };

  return slocInfo;
};

let pendingAmendRequestId = 0;
const pendingAmendSlocAtom = atom(async get => {
  const selectedFiles = get(selectedFilesAtom);
  return fetchPendingAmendSloc(get, selectedFiles, pendingAmendRequestId++);
});

/**
 * FETCH PENDING SLOC
 */
const fetchPendingSloc = async (
  get: Getter,
  includedFiles: string[],
  requestId: number,
): Promise<SlocInfo | undefined> => {
  // this atom makes use of the fact that jotai will only use the most recently created request (ignoring older requests)
  // to avoid race conditions when the response from an older request is sent after a newer one
  // so for example:
  // pendingRequestId A (slow) => Server (sleeps 5 sec)
  // pendingRequestId B (fast) => Server responds immediately, client updates
  // pendingRequestId A (slow) => Server responds, client ignores

  // we don't want to fetch the pending changes if the page is hidden
  const pageIsHidden = get(pageVisibility) === 'hidden';
  const commits = get(commitInfoViewCurrentCommits);

  if (pageIsHidden || commits == null || commits.length > 1) {
    return undefined;
  }

  const [commit] = commits;
  if (commit.totalFileCount > MAX_FILES_ALLOWED_FOR_DIFF_STAT) {
    return undefined;
  }

  if (includedFiles.length > MAX_FILES_ALLOWED_FOR_DIFF_STAT) {
    return undefined;
  }

  if (includedFiles.length === 0) {
    return {sloc: 0, strictSloc: 0};
  }

  serverAPI.postMessage({
    type: 'fetchPendingSignificantLinesOfCode',
    hash: commit.hash,
    includedFiles,
    requestId,
  });

  const pendingLocData = await serverAPI
    .nextMessageMatching(
      'fetchedPendingSignificantLinesOfCode',
      message => message.requestId === requestId && message.hash === commit.hash,
    )
    .then(result => ({
      sloc: result.result.value?.linesOfCode,
      strictSloc: result.result.value?.strictLinesOfCode,
    }));

  return pendingLocData;
};

let pendingRequestId = 0;

const pendingChangesSlocAtom = atom(async get => {
  const selectedFiles = get(selectedFilesAtom);
  return fetchPendingSloc(get, selectedFiles, pendingRequestId++);
});

const pendingAmendChangesSloc = loadable(pendingAmendSlocAtom);
const pendingChangesSloc = loadable(pendingChangesSlocAtom);

function useFetchWithPrevious(
  atom: Atom<Loadable<Promise<SlocInfo | undefined>>>,
): SlocInfo | undefined {
  const previous = useRef<SlocInfo | undefined>(undefined);
  const results = useAtomValue(atom);
  if (results.state === 'hasError') {
    throw results.error;
  }
  if (results.state === 'loading') {
    //using the previous value in the loading state to avoid flickering / jankiness in the UI
    return previous.current;
  }

  previous.current = results.data;

  return results.data;
}

export function useFetchSignificantLinesOfCode(commit: CommitInfo) {
  return useAtomValue(commitSloc(commit.hash));
}

export function useFetchPendingSignificantLinesOfCode() {
  return useFetchWithPrevious(pendingChangesSloc);
}

export function useFetchPendingAmendSignificantLinesOfCode() {
  return useFetchWithPrevious(pendingAmendChangesSloc);
}
