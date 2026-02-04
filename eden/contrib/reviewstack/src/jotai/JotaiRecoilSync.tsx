/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {
  gitHubCommitID,
  gitHubOrgAndRepo,
  gitHubPullRequest,
  gitHubPullRequestSelectedVersionIndex,
  gitHubPullRequestVersions,
} from '../recoil';
import {
  gitHubCommitIDAtom,
  gitHubOrgAndRepoAtom,
  gitHubPullRequestAtom,
  gitHubPullRequestSelectedVersionIndexAtom,
  gitHubPullRequestVersionsAtom,
} from './atoms';
import {useAtomValue, useSetAtom} from 'jotai';
import {useEffect} from 'react';
import {useRecoilValue, useRecoilValueLoadable, useSetRecoilState} from 'recoil';

/**
 * Synchronizes state between Jotai and Recoil during the migration period.
 *
 * This component provides:
 * - Jotai -> Recoil sync: For atoms where components now use Jotai but Recoil
 *   selectors still depend on the Recoil atoms
 * - Recoil -> Jotai sync: For complex selectors that remain in Recoil but whose
 *   values are needed by Jotai-based components
 *
 * Can be removed once all Recoil selectors are migrated to Jotai.
 */
export function JotaiRecoilSync(): null {
  // Jotai -> Recoil sync for orgAndRepo
  // All component consumers now use Jotai, but Recoil selectors like
  // gitHubClient still depend on the Recoil atom
  const jotaiOrgAndRepo = useAtomValue(gitHubOrgAndRepoAtom);
  const setOrgAndRepoRecoil = useSetRecoilState(gitHubOrgAndRepo);

  useEffect(() => {
    setOrgAndRepoRecoil(jotaiOrgAndRepo);
  }, [jotaiOrgAndRepo, setOrgAndRepoRecoil]);

  // Jotai -> Recoil sync for pull request
  // All component consumers now use Jotai, but Recoil selectors like
  // gitHubPullRequestCommits, gitHubPullRequestReviewThreads still depend on the Recoil atom
  const pullRequest = useAtomValue(gitHubPullRequestAtom);
  const setPullRequest = useSetRecoilState(gitHubPullRequest);

  useEffect(() => {
    setPullRequest(pullRequest);
  }, [pullRequest, setPullRequest]);

  // Jotai -> Recoil sync for commit ID
  // CommitView sets via Jotai, Recoil selectors depend on Recoil atom
  const commitID = useAtomValue(gitHubCommitIDAtom);
  const setCommitID = useSetRecoilState(gitHubCommitID);

  useEffect(() => {
    setCommitID(commitID);
  }, [commitID, setCommitID]);

  // Recoil -> Jotai sync for versions
  // gitHubPullRequestVersions is a complex computed selector that depends on many
  // Recoil selectors. We sync its value to the Jotai atom for component consumers.
  // Use loadable to avoid throwing during async loading.
  const recoilVersionsLoadable = useRecoilValueLoadable(gitHubPullRequestVersions);
  const setVersionsAtom = useSetAtom(gitHubPullRequestVersionsAtom);

  useEffect(() => {
    if (recoilVersionsLoadable.state === 'hasValue') {
      setVersionsAtom(recoilVersionsLoadable.contents);
    }
  }, [recoilVersionsLoadable, setVersionsAtom]);

  // Bidirectional sync for selectedVersionIndex
  // - Recoil -> Jotai: When versions load, Recoil computes the default (latest version)
  // - Jotai -> Recoil: When user selects a version, Jotai updates and Recoil selectors
  //   like gitHubPullRequestIsViewingLatest need the updated value
  const recoilSelectedVersionIndex = useRecoilValue(gitHubPullRequestSelectedVersionIndex);
  const jotaiSelectedVersionIndex = useAtomValue(gitHubPullRequestSelectedVersionIndexAtom);
  const setSelectedVersionIndexAtom = useSetAtom(gitHubPullRequestSelectedVersionIndexAtom);
  const setSelectedVersionIndexRecoil = useSetRecoilState(gitHubPullRequestSelectedVersionIndex);

  useEffect(() => {
    // Sync Recoil -> Jotai for initial default value
    setSelectedVersionIndexAtom(recoilSelectedVersionIndex);
  }, [recoilSelectedVersionIndex, setSelectedVersionIndexAtom]);

  useEffect(() => {
    // Sync Jotai -> Recoil when user changes selection
    setSelectedVersionIndexRecoil(jotaiSelectedVersionIndex);
  }, [jotaiSelectedVersionIndex, setSelectedVersionIndexRecoil]);

  return null;
}
