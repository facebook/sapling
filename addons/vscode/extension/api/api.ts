/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {RepositoryContext} from 'isl-server/src/serverTypes';
import type {VSCodeServerPlatform} from '../vscodePlatform';
import type {VSCodeReposList} from '../VSCodeRepo';
import type {SaplingExtensionApi, SaplingRepository} from './types';

export function makeExtensionApi(
  platform: VSCodeServerPlatform,
  ctx: RepositoryContext,
  reposList: VSCodeReposList,
): SaplingExtensionApi {
  return {
    version: '1',
    getActiveRepositories() {
      return reposList.getCurrentActiveRepos();
    },
    onDidChangeActiveRepositories(cb: (repositories: Array<SaplingRepository>) => unknown) {
      return reposList.observeActiveRepos(repos => {
        return cb(repos);
      });
    },
    getRepositoryForPath(path: string): SaplingRepository | undefined {
      return reposList.repoForPath(path);
    },
  };
}
