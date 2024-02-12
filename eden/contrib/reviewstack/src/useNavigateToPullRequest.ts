/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import URLFor from './URLFor';
import {gitHubOrgAndRepo} from './recoil';
import useNavigate from './useNavigate';
import {useCallback} from 'react';
import {useRecoilValue} from 'recoil';

export default function useNavigateToPullRequest(): (number: number) => void {
  const navigate = useNavigate();
  const {org, repo} = useRecoilValue(gitHubOrgAndRepo) ?? {};

  return useCallback(
    (number: number) => {
      if (org != null && repo != null) {
        navigate(URLFor.pullRequest({org, repo, number}));
      }
    },
    [navigate, org, repo],
  );
}
