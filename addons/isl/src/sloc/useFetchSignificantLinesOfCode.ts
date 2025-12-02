/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Atom, Getter} from 'jotai';
import type {Loadable} from 'jotai/vanilla/utils/loadable';
import type {CommitInfo, RepoRelativePath, SlocInfo} from '../types';

import {atom, useAtomValue} from 'jotai';
import {loadable} from 'jotai/utils';
import {useEffect, useMemo, useRef, useState} from 'react';
import serverAPI from '../ClientToServerAPI';
import {commitInfoViewCurrentCommits} from '../CommitInfoView/CommitInfoState';
import {getGeneratedFilesFrom} from '../GeneratedFile';
import {pageVisibility} from '../codeReview/CodeReviewInfo';
import {atomFamilyWeak, lazyAtom} from '../jotaiUtils';
import {isFullyOrPartiallySelected} from '../partialSelection';
import {uncommittedChangesWithPreviews} from '../previews';
import {commitByHash} from '../serverAPIState';
import {GeneratedStatus} from '../types';
import {MAX_FILES_ALLOWED_FOR_DIFF_STAT} from './diffStatConstants';

const getGeneratedFiles = (files: ReadonlyArray<RepoRelativePath>): Array<RepoRelativePath> => {
  const generatedStatuses = getGeneratedFilesFrom(files);

  return files.reduce<string[]>((filtered, path) => {
    // check if the file should be excluded
    // the __generated__ pattern is included in the exclusions, so we don't need to include it here
    if (path.match(/__generated__/) || generatedStatuses[path] === GeneratedStatus.Generated) {
      filtered.push(path);
    }

    return filtered;
  }, []);
};

const filterGeneratedFiles = (files: ReadonlyArray<RepoRelativePath>): Array<RepoRelativePath> => {
  const generatedStatuses = getGeneratedFilesFrom(files);

  return files.filter(
    path => !path.match(/__generated__/) && generatedStatuses[path] !== GeneratedStatus.Generated,
  );
};

async function fetchSignificantLinesOfCode(
  commit: Readonly<CommitInfo>,
  additionalFilesToExclude: ReadonlyArray<RepoRelativePath> = [],
  getExcludedFiles: (
    files: ReadonlyArray<RepoRelativePath>,
  ) => Array<RepoRelativePath> = getGeneratedFiles,
): Promise<SlocInfo> {
  const filesToQueryGeneratedStatus = commit.filePathsSample;
  const excludedFiles = getExcludedFiles(filesToQueryGeneratedStatus);

  serverAPI.postMessage({
    type: 'fetchSignificantLinesOfCode',
    hash: commit.hash,
    excludedFiles: [...excludedFiles, ...additionalFilesToExclude],
  });

  const slocData = await serverAPI
    .nextMessageMatching('fetchedSignificantLinesOfCode', message => message.hash === commit.hash)
    .then(result => ({
      sloc: result.result.value,
    }));

  return slocData;
}

const commitSlocFamily = atomFamilyWeak((hash: string) => {
  return lazyAtom(async get => {
    const commit = get(commitByHash(hash));
    if (commit == null) {
      return undefined;
    }
    if (commit.totalFileCount > MAX_FILES_ALLOWED_FOR_DIFF_STAT) {
      return undefined;
    }
    if (commit.optimisticRevset != null) {
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
  if (commit.totalFileCount > MAX_FILES_ALLOWED_FOR_DIFF_STAT || commit.optimisticRevset != null) {
    return undefined;
  }

  const filteredFiles = filterGeneratedFiles(includedFiles);
  if (filteredFiles.length > MAX_FILES_ALLOWED_FOR_DIFF_STAT) {
    return undefined;
  }

  if (filteredFiles.length === 0) {
    return {sloc: 0};
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
    includedFiles: filteredFiles,
    requestId,
  });

  const pendingLoc = await serverAPI
    .nextMessageMatching(
      'fetchedPendingAmendSignificantLinesOfCode',
      message => message.requestId === requestId && message.hash === commit.hash,
    )
    .then(result => ({
      sloc: result.result.value,
    }));

  if (unselectedCommittedSlocInfo === undefined) {
    return pendingLoc;
  }

  if (pendingLoc === undefined) {
    return unselectedCommittedSlocInfo;
  }

  const slocInfo = {
    sloc: (unselectedCommittedSlocInfo.sloc ?? 0) + (pendingLoc.sloc ?? 0),
  };

  return slocInfo;
};

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

  const filteredFiles = filterGeneratedFiles(includedFiles);
  if (filteredFiles.length > MAX_FILES_ALLOWED_FOR_DIFF_STAT) {
    return undefined;
  }

  if (filteredFiles.length === 0) {
    return {sloc: 0};
  }

  serverAPI.postMessage({
    type: 'fetchPendingSignificantLinesOfCode',
    hash: commit.hash,
    includedFiles: filteredFiles,
    requestId,
  });

  const pendingLocData = await serverAPI
    .nextMessageMatching(
      'fetchedPendingSignificantLinesOfCode',
      message => message.requestId === requestId && message.hash === commit.hash,
    )
    .then(result => ({
      sloc: result.result.value,
    }));

  return pendingLocData;
};

function useFetchWithPrevious(atom: Atom<Loadable<Promise<SlocInfo | undefined>>>): {
  slocInfo: SlocInfo | undefined;
  isLoading: boolean;
} {
  const previous = useRef<SlocInfo | undefined>(undefined);
  const results = useAtomValue(atom);
  if (results.state === 'hasError') {
    throw results.error;
  }
  if (results.state === 'loading') {
    //using the previous value in the loading state to avoid flickering / jankiness in the UI
    return {slocInfo: previous.current, isLoading: true};
  }

  previous.current = results.data;

  return {slocInfo: results.data, isLoading: false};
}

export function useFetchSignificantLinesOfCode(commit: CommitInfo) {
  const loadableAtom = loadable(commitSlocFamily(commit.hash));
  const result = useAtomValue(loadableAtom);

  if (result.state === 'hasError') {
    throw result.error;
  }

  if (result.state === 'loading') {
    return {slocInfo: undefined, isLoading: true};
  }

  return {slocInfo: result.data, isLoading: false};
}

// Debounce delay for SLOC requests to prevent spamming when many files are selected quickly
const DEBOUNCE_DELAY_MS = 300;

// Hook that debounces an atom value
function useDebouncedAtomValue<T>(sourceAtom: Atom<T>, debounceMs: number): T {
  const currentValue = useAtomValue(sourceAtom);
  const [debouncedValue, setDebouncedValue] = useState(currentValue);
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    if (timeoutRef.current) {
      clearTimeout(timeoutRef.current);
    }

    timeoutRef.current = setTimeout(() => {
      setDebouncedValue(currentValue);
    }, debounceMs);

    return () => {
      if (timeoutRef.current) {
        clearTimeout(timeoutRef.current);
      }
    };
  }, [currentValue, debounceMs]);

  return debouncedValue;
}

let pendingRequestId = 0;
export function useFetchPendingSignificantLinesOfCode() {
  // Debounce selected files to prevent spamming SLOC requests
  const debouncedSelectedFiles = useDebouncedAtomValue(selectedFilesAtom, DEBOUNCE_DELAY_MS);

  // Use a derived atom that depends on the debounced value
  const debouncedAtom = useMemo(() => {
    return atom(get => {
      // Force the atom to use the debounced value by creating a dependency
      // Note: we can't pass debouncedSelectedFiles directly to the atom,
      // so we create a new fetch call with it
      return fetchPendingSloc(get, debouncedSelectedFiles, pendingRequestId++);
    });
  }, [debouncedSelectedFiles]);

  const loadableAtom = loadable(debouncedAtom);
  return useFetchWithPrevious(loadableAtom);
}

let pendingAmendRequestId = 0;
export function useFetchPendingAmendSignificantLinesOfCode() {
  // Debounce selected files to prevent spamming SLOC requests
  const debouncedSelectedFiles = useDebouncedAtomValue(selectedFilesAtom, DEBOUNCE_DELAY_MS);

  // Use a derived atom that depends on the debounced value
  const debouncedAtom = useMemo(() => {
    return atom(get => {
      return fetchPendingAmendSloc(get, debouncedSelectedFiles, pendingAmendRequestId++);
    });
  }, [debouncedSelectedFiles]);

  const loadableAtom = loadable(debouncedAtom);
  return useFetchWithPrevious(loadableAtom);
}
