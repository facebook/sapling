/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Repository} from '../Repository';
import type {Logger} from '../logger';
import type {RepoInfo, RepositoryError} from 'isl/src/types';

import {__TEST__} from '../RepositoryCache';
import {mockLogger} from 'shared/testUtils';
import {defer} from 'shared/utils';

const {RepositoryCache} = __TEST__;

class SimpleMockRepositoryImpl {
  static getRepoInfo(command: string, _logger: Logger, cwd: string): Promise<RepoInfo> {
    let data;
    if (cwd.includes('/path/to/repo')) {
      data = {
        repoRoot: '/path/to/repo',
        dotdir: '/path/to/repo/.sl',
      };
    } else if (cwd.includes('/path/to/anotherrepo')) {
      data = {
        repoRoot: '/path/to/anotherrepo',
        dotdir: '/path/to/anotherrepo/.sl',
      };
    } else {
      return Promise.resolve({type: 'cwdNotARepository', cwd} as RepositoryError);
    }
    return Promise.resolve({
      type: 'success',
      command,
      pullRequestDomain: undefined,
      preferredSubmitCommand: 'pr',
      codeReviewSystem: {type: 'unknown'},
      ...data,
    });
  }
  constructor(public info: RepoInfo, public logger: Logger) {}

  dispose = jest.fn();
}
const SimpleMockRepository = SimpleMockRepositoryImpl as unknown as typeof Repository;

describe('RepositoryCache', () => {
  it('Provides repository references that resolve', async () => {
    const cache = new RepositoryCache(SimpleMockRepository);
    const ref = cache.getOrCreate('sl', mockLogger, '/path/to/repo/cwd');

    const repo = await ref.promise;
    expect(repo).toEqual(
      expect.objectContaining({
        info: expect.objectContaining({
          repoRoot: '/path/to/repo',
          dotdir: '/path/to/repo/.sl',
        }),
      }),
    );

    ref.unref();
  });

  it('Gives error for paths without repos', async () => {
    const cache = new RepositoryCache(SimpleMockRepository);
    const ref = cache.getOrCreate('sl', mockLogger, '/some/invalid/repo');

    const repo = await ref.promise;
    expect(repo).toEqual({type: 'cwdNotARepository', cwd: '/some/invalid/repo'});

    ref.unref();
  });

  it('Disposes repositories', async () => {
    const cache = new RepositoryCache(SimpleMockRepository);
    const ref = cache.getOrCreate('sl', mockLogger, '/path/to/repo/cwd');

    const repo = await ref.promise;
    const disposeFunc = (repo as Repository).dispose;

    expect(cache.numberOfActiveServers()).toBe(1);

    ref.unref();
    expect(disposeFunc).toHaveBeenCalledTimes(1);
    expect(cache.numberOfActiveServers()).toBe(0);
  });

  it('Can dispose references before the repo promise has resolved', async () => {
    const cache = new RepositoryCache(SimpleMockRepository);
    const ref = cache.getOrCreate('sl', mockLogger, '/path/to/repo/cwd');

    ref.unref();

    const repo = await ref.promise;
    // even though this would be a valid repo, by disposing the ref before it is created,
    // we prevent creating a repo.
    expect(repo).toEqual({type: 'unknownError', error: expect.anything()});
  });

  it('shares repositories under the same cwd', async () => {
    const cache = new RepositoryCache(SimpleMockRepository);
    const ref1 = cache.getOrCreate('sl', mockLogger, '/path/to/repo');
    const repo1 = await ref1.promise;

    const ref2 = cache.getOrCreate('sl', mockLogger, '/path/to/repo');
    const repo2 = await ref2.promise;

    expect(cache.numberOfActiveServers()).toBe(2);

    expect(repo1).toBe(repo2);

    ref1.unref();
    ref2.unref();
  });

  it('shares repositories under the same repo', async () => {
    const cache = new RepositoryCache(SimpleMockRepository);
    const ref1 = cache.getOrCreate('sl', mockLogger, '/path/to/repo/cwd1');
    const repo1 = await ref1.promise;

    const ref2 = cache.getOrCreate('sl', mockLogger, '/path/to/repo/cwd2');
    const repo2 = await ref2.promise;

    expect(cache.numberOfActiveServers()).toBe(2);

    expect(repo1).toBe(repo2);

    ref1.unref();
    ref2.unref();
  });

  it('does not share repositories under different cwds', async () => {
    const cache = new RepositoryCache(SimpleMockRepository);
    const ref1 = cache.getOrCreate('sl', mockLogger, '/path/to/repo/cwd');
    const ref2 = cache.getOrCreate('sl', mockLogger, '/path/to/anotherrepo/cwd');

    const repo1 = await ref1.promise;
    const repo2 = await ref2.promise;

    expect(repo1).not.toBe(repo2);

    ref1.unref();
    ref2.unref();
  });

  it('reference counts and disposes after 0 refs', async () => {
    const cache = new RepositoryCache(SimpleMockRepository);
    const ref1 = cache.getOrCreate('sl', mockLogger, '/path/to/repo/cwd1');
    const ref2 = cache.getOrCreate('sl', mockLogger, '/path/to/repo/cwd2');

    const repo = await ref1.promise;
    await ref2.promise;
    expect(cache.numberOfActiveServers()).toBe(2);

    const disposeFunc = (repo as Repository).dispose;

    ref1.unref();
    expect(disposeFunc).not.toHaveBeenCalled();
    ref2.unref();
    expect(disposeFunc).toHaveBeenCalledTimes(1);

    expect(cache.numberOfActiveServers()).toBe(0);
  });

  it('does not re-use diposed repos', async () => {
    const cache = new RepositoryCache(SimpleMockRepository);
    const ref1 = cache.getOrCreate('sl', mockLogger, '/path/to/repo/cwd1');

    const repo1 = await ref1.promise;
    ref1.unref();

    const ref2 = cache.getOrCreate('sl', mockLogger, '/path/to/repo/cwd2');
    const repo2 = await ref2.promise;

    expect(cache.numberOfActiveServers()).toBe(1);

    expect(repo1).not.toBe(repo2);
    expect((repo1 as Repository).dispose).toHaveBeenCalledTimes(1);
    expect((repo2 as Repository).dispose).not.toHaveBeenCalled();

    ref2.unref();
  });

  it('prefix matching repos are treated separately', async () => {
    const cache = new RepositoryCache(SimpleMockRepository);
    const ref1 = cache.getOrCreate('sl', mockLogger, '/path/to/repo');
    await ref1.promise;

    expect(cache.cachedRepositoryForPath('/path/to/repo')).not.toEqual(undefined);
    expect(cache.cachedRepositoryForPath('/path/to/repo/')).not.toEqual(undefined);
    expect(cache.cachedRepositoryForPath('/path/to/repo/foo')).not.toEqual(undefined);
    // this is actually different repo
    expect(cache.cachedRepositoryForPath('/path/to/repo-1')).toEqual(undefined);

    ref1.unref();
  });

  it('only creates one Repository even when racing lookups', async () => {
    const repoInfo = {
      type: 'success',
      command: 'sl',
      pullRequestDomain: undefined,
      codeReviewSystem: {type: 'unknown'},
      repoRoot: '/path/to/repo',
      dotdir: '/path/to/repo/.sl',
    } as RepoInfo;

    // two different fake async fetches for RepoInfo
    const p1 = defer<RepoInfo>();
    const p2 = defer<RepoInfo>();

    class BlockingMockRepository {
      static getRepoInfo(_command: string, _logger: Logger, cwd: string): Promise<RepoInfo> {
        if (cwd === '/path/to/repo/cwd1') {
          return p1.promise;
        } else if (cwd === '/path/to/repo/cwd2') {
          return p2.promise;
        }
        throw new Error('unknown repo');
      }
      constructor(public info: RepoInfo, public logger: Logger) {}

      dispose = jest.fn();
    }

    const cache = new RepositoryCache(BlockingMockRepository as unknown as typeof Repository);
    // start looking up repoInfos at the same time
    const ref1 = cache.getOrCreate('sl', mockLogger, '/path/to/repo/cwd1');
    const ref2 = cache.getOrCreate('sl', mockLogger, '/path/to/repo/cwd2');

    p2.resolve({...repoInfo});
    const repo2 = await ref2.promise;

    p1.resolve({...repoInfo});
    const repo1 = await ref1.promise;

    // we end up with the same repo
    expect(repo1).toBe(repo2);

    ref1.unref();
    ref2.unref();
  });
});
