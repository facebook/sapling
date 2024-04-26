/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {RepoInfo} from './types';

import {atomResetOnDepChange} from './jotaiUtils';
import {initialParams} from './urlParams';
import {atom} from 'jotai';

export const repositoryData = atom<{info?: RepoInfo; cwd?: string}>({});

/** "cwd" is not always the repo root. It can be a path inside the repo. */
export const serverCwd = atom<string>(get => {
  const data = get(repositoryData);
  if (data.info?.type === 'cwdNotARepository') {
    return data.info.cwd;
  }
  return data?.cwd ?? initialParams.get('cwd') ?? '';
});

const repoRoot = atom(get => {
  const data = get(repositoryData);
  return data.info?.type === 'success' ? data.info.repoRoot : '';
});

/**
 * A string of repo root and the "cwd". Note a same "cwd" does not infer the same repo,
 * when there are nested (ex. submodule) repos.
 */
const repoRootAndCwd = atom<string>(get => `${get(serverCwd)}\n${get(repoRoot)}`);

/** A specific version of `atomResetOnDepChange`. */
export function atomResetOnCwdChange<T>(defaultValue: T) {
  return atomResetOnDepChange(defaultValue, repoRootAndCwd);
}
