/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {
  gitHubOrgAndRepo,
  gitHubPullRequest,
  gitHubPullRequestComparableVersions,
  gitHubPullRequestSelectedVersionIndex,
} from '../recoil';
import {
  gitHubOrgAndRepoAtom,
  gitHubPullRequestAtom,
  gitHubPullRequestComparableVersionsAtom,
  gitHubPullRequestSelectedVersionIndexAtom,
  gitHubPullRequestVersionsAtom,
} from './atoms';
import {useAtomValue, useSetAtom} from 'jotai';
import {loadable} from 'jotai/utils';
import {useEffect, useMemo, useRef} from 'react';
import {useSetRecoilState} from 'recoil';

/**
 * Synchronizes state between Jotai and Recoil during the migration period.
 *
 * This component provides:
 * - Jotai -> Recoil sync: For atoms where components now use Jotai but Recoil
 *   selectors still depend on the Recoil atoms
 * - Computes version-based defaults from Jotai and syncs to Recoil
 *
 * Can be removed once all Recoil selectors are migrated to Jotai.
 */
export function JotaiRecoilSync(): null {
  // Jotai -> Recoil sync for orgAndRepo
  // All component consumers now use Jotai, but Recoil selectors like
  // gitHubClientForParams still depend on the Recoil atom
  const jotaiOrgAndRepo = useAtomValue(gitHubOrgAndRepoAtom);
  const setOrgAndRepoRecoil = useSetRecoilState(gitHubOrgAndRepo);

  useEffect(() => {
    setOrgAndRepoRecoil(jotaiOrgAndRepo);
  }, [jotaiOrgAndRepo, setOrgAndRepoRecoil]);

  // Jotai -> Recoil sync for pull request
  // Keep this sync for gitHubPullRequestForParams which still uses Recoil
  const pullRequest = useAtomValue(gitHubPullRequestAtom);
  const setPullRequest = useSetRecoilState(gitHubPullRequest);

  useEffect(() => {
    setPullRequest(pullRequest);
  }, [pullRequest, setPullRequest]);

  // Track the pull request ID to detect when we switch to a different PR
  const lastPullRequestId = useRef<string | null>(null);
  const currentPullRequestId = pullRequest?.id ?? null;

  // Load versions from Jotai (async atom)
  const loadableVersionsAtom = useMemo(() => loadable(gitHubPullRequestVersionsAtom), []);
  const versionsLoadable = useAtomValue(loadableVersionsAtom);

  // Setters for Jotai atoms
  const setSelectedVersionIndexAtom = useSetAtom(gitHubPullRequestSelectedVersionIndexAtom);
  const setComparableVersionsAtom = useSetAtom(gitHubPullRequestComparableVersionsAtom);

  // Setters for Recoil atoms
  const setSelectedVersionIndexRecoil = useSetRecoilState(gitHubPullRequestSelectedVersionIndex);
  const setComparableVersionsRecoil = useSetRecoilState(gitHubPullRequestComparableVersions);

  // When versions load (or when switching to a different PR), compute defaults
  useEffect(() => {
    if (versionsLoadable.state !== 'hasData') {
      return;
    }

    const versions = versionsLoadable.data;
    if (versions.length === 0) {
      return;
    }

    // Detect if we switched to a different PR
    const switchedPR = currentPullRequestId !== lastPullRequestId.current;
    lastPullRequestId.current = currentPullRequestId;

    // Only set defaults when switching to a different PR (or initial load)
    // This prevents resetting user selection when versions update
    if (switchedPR || currentPullRequestId === null) {
      // Compute default selected version index (latest version)
      const defaultVersionIndex = versions.length - 1;
      setSelectedVersionIndexAtom(defaultVersionIndex);
      setSelectedVersionIndexRecoil(defaultVersionIndex);

      // Compute default comparable versions
      const latestVersion = versions[defaultVersionIndex];
      if (latestVersion != null) {
        const defaultComparableVersions = {
          beforeCommitID: latestVersion.baseParent,
          afterCommitID: latestVersion.headCommit,
        };
        setComparableVersionsAtom(defaultComparableVersions);
        setComparableVersionsRecoil(defaultComparableVersions);
      }
    }
  }, [
    versionsLoadable,
    currentPullRequestId,
    setSelectedVersionIndexAtom,
    setSelectedVersionIndexRecoil,
    setComparableVersionsAtom,
    setComparableVersionsRecoil,
  ]);

  // Sync Jotai -> Recoil when user changes selection
  const jotaiSelectedVersionIndex = useAtomValue(gitHubPullRequestSelectedVersionIndexAtom);
  const jotaiComparableVersions = useAtomValue(gitHubPullRequestComparableVersionsAtom);

  useEffect(() => {
    setSelectedVersionIndexRecoil(jotaiSelectedVersionIndex);
  }, [jotaiSelectedVersionIndex, setSelectedVersionIndexRecoil]);

  useEffect(() => {
    if (jotaiComparableVersions != null) {
      setComparableVersionsRecoil(jotaiComparableVersions);
    }
  }, [jotaiComparableVersions, setComparableVersionsRecoil]);

  return null;
}
