/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import Link from './Link';
import URLFor from './URLFor';
import {gitHubOrgAndRepo} from './recoil';
import {useRecoilValue} from 'recoil';

type Props = {
  children: React.ReactElement;
  number: number;
};

export default function PullRequestLink({children, number}: Props): React.ReactElement {
  const {org, repo} = useRecoilValue(gitHubOrgAndRepo) ?? {};

  if (org == null || repo == null) {
    return children;
  }

  return <Link href={URLFor.pullRequest({org, repo, number})}>{children}</Link>;
}
