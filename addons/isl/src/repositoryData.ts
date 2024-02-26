/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {RepoInfo} from './types';

import {initialParams} from './urlParams';
import {atom} from 'jotai';

export const repositoryData = atom<{info?: RepoInfo; cwd?: string}>({});

export const serverCwd = atom(get => {
  const data = get(repositoryData);
  if (data.info?.type === 'cwdNotARepository') {
    return data.info.cwd;
  }
  return data?.cwd ?? initialParams.get('cwd') ?? '';
});
