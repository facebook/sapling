/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {GitObjectID} from './github/types';

import Link from './Link';
import URLFor from './URLFor';
import {shortOid} from './utils';

export default function CommitLink({
  org,
  repo,
  oid,
}: {
  org: string;
  repo: string;
  oid: GitObjectID;
}): React.ReactElement {
  return <Link href={URLFor.commit({org, repo, oid})}>{shortOid(oid)}</Link>;
}
