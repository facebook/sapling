/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {RepositoryContext} from 'isl-server/src/serverTypes';
import type {VSCodeServerPlatform} from '../vscodePlatform';
import type {VSCodeReposList} from '../VSCodeRepo';

import {makeExtensionApi} from '../api/api';
import {postMessageToISLWebview} from '../islWebviewPanel';

jest.mock('../islWebviewPanel', () => ({
  postMessageToISLWebview: jest.fn(),
}));

describe('setActiveRepoForCwd', () => {
  beforeEach(() => {
    jest.clearAllMocks();
  });

  const makeApi = (repoForPath: jest.Mock) =>
    makeExtensionApi(
      {} as VSCodeServerPlatform,
      {} as RepositoryContext,
      {
        repoForPath,
      } as unknown as VSCodeReposList,
    );

  it('changes to a repository available in the VS Code workspace', () => {
    const repoForPath = jest.fn(() => ({}));
    const api = makeApi(repoForPath);

    api.setActiveRepoForCwd('/path/to/workspace-repo');

    expect(repoForPath).toHaveBeenCalledWith('/path/to/workspace-repo');
    expect(postMessageToISLWebview).toHaveBeenCalledWith({
      type: 'changeActiveRepo',
      cwd: '/path/to/workspace-repo',
      focusDotCommit: true,
    });
  });

  it('throws and does not change to a repository outside the VS Code workspace', () => {
    const repoForPath = jest.fn(() => undefined);
    const api = makeApi(repoForPath);

    expect(() => api.setActiveRepoForCwd('/path/to/other-repo')).toThrow();

    expect(repoForPath).toHaveBeenCalledWith('/path/to/other-repo');
    expect(postMessageToISLWebview).not.toHaveBeenCalled();
  });
});
