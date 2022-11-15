/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {GitObjectID} from './github/types';

function commit({org, repo, oid}: {org: string; repo: string; oid: GitObjectID}): string {
  return `/${org}/${repo}/commit/${oid}`;
}

function project({org, repo}: {org: string; repo: string}): string {
  return `/${org}/${repo}`;
}

function pullRequest({org, repo, number}: {org: string; repo: string; number: number}): string {
  return `/${org}/${repo}/pull/${number}`;
}

function pulls({org, repo}: {org: string; repo: string}): string {
  return `/${org}/${repo}/pulls`;
}

function defaultAvatar(): string {
  return 'https://avatars.githubusercontent.com/github';
}

export default {
  commit,
  project,
  pullRequest,
  pulls,
  defaultAvatar,
};
