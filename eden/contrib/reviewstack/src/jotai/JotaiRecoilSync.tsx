/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {gitHubOrgAndRepoAtom} from './atoms';
import {gitHubOrgAndRepo} from '../recoil';
import {useSetAtom} from 'jotai';
import {useEffect} from 'react';
import {useRecoilValue} from 'recoil';

/**
 * Synchronizes Recoil state to Jotai atoms during the migration period.
 *
 * This component bridges the gap where:
 * - Components set Recoil atoms (e.g., PullRequestLayout sets gitHubOrgAndRepo)
 * - Other components consume Jotai atoms that depend on those values
 *
 * Can be removed once all setters are migrated to Jotai.
 */
export function JotaiRecoilSync(): null {
  const orgAndRepo = useRecoilValue(gitHubOrgAndRepo);
  const setOrgAndRepoAtom = useSetAtom(gitHubOrgAndRepoAtom);

  useEffect(() => {
    setOrgAndRepoAtom(orgAndRepo);
  }, [orgAndRepo, setOrgAndRepoAtom]);

  return null;
}
