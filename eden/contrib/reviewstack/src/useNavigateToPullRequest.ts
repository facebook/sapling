/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import URLFor from './URLFor';
import {gitHubOrgAndRepoAtom} from './jotai';
import useNavigate from './useNavigate';
import {useAtomValue} from 'jotai';
import {useCallback} from 'react';

export default function useNavigateToPullRequest(): (number: number) => void {
  const navigate = useNavigate();
  const {org, repo} = useAtomValue(gitHubOrgAndRepoAtom) ?? {};

  return useCallback(
    (number: number) => {
      if (org != null && repo != null) {
        navigate(URLFor.pullRequest({org, repo, number}));
      }
    },
    [navigate, org, repo],
  );
}
