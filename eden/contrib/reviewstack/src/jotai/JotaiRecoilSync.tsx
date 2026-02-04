/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {gitHubCommitID, gitHubOrgAndRepo, gitHubPullRequest} from '../recoil';
import {gitHubCommitIDAtom, gitHubOrgAndRepoAtom, gitHubPullRequestAtom} from './atoms';
import {useAtomValue, useSetAtom} from 'jotai';
import {useEffect} from 'react';
import {useRecoilValue, useSetRecoilState} from 'recoil';

/**
 * Synchronizes Recoil state to Jotai atoms during the migration period.
 *
 * This component provides bidirectional sync between Recoil and Jotai:
 * - Recoil -> Jotai: For atoms still set via Recoil (e.g., PullRequestLayout)
 * - Jotai -> Recoil: For atoms migrated to Jotai but with remaining Recoil dependents
 *
 * Can be removed once all setters and consumers are migrated to Jotai.
 */
export function JotaiRecoilSync(): null {
  // Bidirectional sync for orgAndRepo
  // - PullRequestLayout sets via Recoil
  // - CommitView sets via Jotai
  // - gitHubClient (Recoil selector) reads from Recoil
  // - Jotai atoms read from Jotai
  const recoilOrgAndRepo = useRecoilValue(gitHubOrgAndRepo);
  const jotaiOrgAndRepo = useAtomValue(gitHubOrgAndRepoAtom);
  const setOrgAndRepoAtom = useSetAtom(gitHubOrgAndRepoAtom);
  const setOrgAndRepoRecoil = useSetRecoilState(gitHubOrgAndRepo);

  useEffect(() => {
    // Sync Recoil -> Jotai when Recoil has a value and Jotai doesn't (or differs)
    if (recoilOrgAndRepo != null) {
      setOrgAndRepoAtom(recoilOrgAndRepo);
    }
  }, [recoilOrgAndRepo, setOrgAndRepoAtom]);

  useEffect(() => {
    // Sync Jotai -> Recoil when Jotai has a value (for CommitView)
    if (jotaiOrgAndRepo != null) {
      setOrgAndRepoRecoil(jotaiOrgAndRepo);
    }
  }, [jotaiOrgAndRepo, setOrgAndRepoRecoil]);

  // Jotai -> Recoil sync for pull request (migrated to Jotai but Recoil selectors depend on it)
  const pullRequest = useAtomValue(gitHubPullRequestAtom);
  const setPullRequest = useSetRecoilState(gitHubPullRequest);

  useEffect(() => {
    setPullRequest(pullRequest);
  }, [pullRequest, setPullRequest]);

  // Jotai -> Recoil sync for commit ID (CommitView sets via Jotai, Recoil selectors depend on it)
  const commitID = useAtomValue(gitHubCommitIDAtom);
  const setCommitID = useSetRecoilState(gitHubCommitID);

  useEffect(() => {
    setCommitID(commitID);
  }, [commitID, setCommitID]);

  return null;
}
