/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ResolveCommandConflictOutput} from '../Repository';
import type {ValidatedRepoInfo} from 'isl/src/types';

import {extractGithubRepoInfoFromUrl, Repository} from '../Repository';
import * as execa from 'execa';
import * as fsUtils from 'shared/fs';
import {clone, mockLogger, nextTick} from 'shared/testUtils';

jest.mock('execa', () => {
  return jest.fn();
});

jest.mock('../WatchForChanges', () => {
  class MockWatchForChanges {
    dispose = jest.fn();
  }
  return {WatchForChanges: MockWatchForChanges};
});

describe('Repository', () => {
  it('setting command name', async () => {
    const spy = jest.spyOn(execa, 'default');
    await Repository.getRepoInfo('slb', mockLogger, '/path/to/cwd');
    expect(spy).toHaveBeenCalledWith('slb', expect.arrayContaining(['root']), expect.anything());
  });

  it('extracting github repo info', async () => {
    jest.spyOn(execa, 'default').mockImplementation(((_cmd: string, args: Array<string>) => {
      const argStr = args?.join(' ');
      if (argStr.startsWith('config paths.default')) {
        return {stdout: 'https://github.com/myUsername/myRepo.git'};
      } else if (argStr.startsWith('config github.pull_request_domain')) {
        return {stdout: 'github.com'};
      } else if (argStr.startsWith('root --dotdir')) {
        return {stdout: '/path/to/myRepo/.sl'};
      } else if (argStr.startsWith('root')) {
        return {stdout: '/path/to/myRepo'};
      }
      return {stdout: ''};
    }) as unknown as typeof execa.default);
    const info = (await Repository.getRepoInfo(
      'sl',
      mockLogger,
      '/path/to/cwd',
    )) as ValidatedRepoInfo;
    const repo = new Repository(info, mockLogger);
    expect(repo.info).toEqual({
      command: 'sl',
      repoRoot: '/path/to/myRepo',
      dotdir: '/path/to/myRepo/.sl',
      codeReviewSystem: {
        type: 'github',
        owner: 'myUsername',
        repo: 'myRepo',
      },
      pullRequestDomain: 'github.com',
    });
  });

  it('extracting repo info', async () => {
    jest.spyOn(execa, 'default').mockImplementation(((_cmd: string, args: Array<string>) => {
      const argStr = args?.join(' ');
      if (argStr.startsWith('config paths.default')) {
        return {stdout: 'mononoke://0.0.0.0/fbsource'};
      } else if (argStr.startsWith('config github.pull_request_domain')) {
        throw new Error('');
      } else if (argStr.startsWith('root --dotdir')) {
        return {stdout: '/path/to/myRepo/.sl'};
      } else if (argStr.startsWith('root')) {
        return {stdout: '/path/to/myRepo'};
      }
      return {stdout: ''};
    }) as unknown as typeof execa.default);
    const info = (await Repository.getRepoInfo(
      'sl',
      mockLogger,
      '/path/to/cwd',
    )) as ValidatedRepoInfo;
    const repo = new Repository(info, mockLogger);
    expect(repo.info).toEqual({
      command: 'sl',
      repoRoot: '/path/to/myRepo',
      dotdir: '/path/to/myRepo/.sl',
      codeReviewSystem: {
        type: 'phabricator',
        repo: 'fbsource',
      },
      pullRequestDomain: undefined,
    });
  });

  describe('merge conflicts', () => {
    const repoInfo: ValidatedRepoInfo = {
      command: 'sl',
      dotdir: '/path/to/repo/.sl',
      repoRoot: '/path/to/repo',
      codeReviewSystem: {type: 'unknown'},
      pullRequestDomain: undefined,
    };
    const NOT_IN_CONFLICT: ResolveCommandConflictOutput = [
      {
        command: null,
        conflicts: [],
        pathconflicts: [],
      },
    ];

    const conflictFileData = (contents: string) => ({
      contents,
      exists: true,
      isexec: false,
      issymlink: false,
    });
    const MARK_IN = '<'.repeat(7) + ` dest:   aaaaaaaaaaaa - unixname: Commit A`;
    const MARK_OUT = '>'.repeat(7) + ` source: bbbbbbbbbbbb - unixname: Commit B`;
    const MARK_BASE_START = `||||||| base`;
    const MARK_BASE_END = `=======`;

    const MOCK_CONFLICT: ResolveCommandConflictOutput = [
      {
        command: 'rebase',
        command_details: {
          cmd: 'rebase',
          to_abort: 'rebase --abort',
          to_continue: 'rebase --continue',
        },
        conflicts: [
          {
            base: conflictFileData('hello\nworld\n'),
            local: conflictFileData('hello\nworld - modified 1\n'),
            other: conflictFileData('hello\nworld - modified 2\n'),
            output: conflictFileData(
              `\
hello
${MARK_IN}
world - modified 1
${MARK_BASE_START}
world
${MARK_BASE_END}
modified 2
${MARK_OUT}
`,
            ),
            path: 'file1.txt',
          },
          {
            base: conflictFileData('hello\nworld\n'),
            local: conflictFileData('hello\nworld - modified 1\n'),
            other: conflictFileData('hello\nworld - modified 2\n'),
            output: conflictFileData(
              `\
hello
${MARK_IN}
world - modified 1
${MARK_BASE_START}
world
${MARK_BASE_END}
modified 2
${MARK_OUT}
`,
            ),
            path: 'file2.txt',
          },
        ],
        pathconflicts: [],
      },
    ];

    // same as MOCK_CONFLICT, but without any data for file1.txt
    const MOCK_CONFLICT_WITH_FILE1_RESOLVED: ResolveCommandConflictOutput = clone(MOCK_CONFLICT);
    MOCK_CONFLICT_WITH_FILE1_RESOLVED[0].conflicts.splice(0, 1);

    // these mock values are returned by execa / fs mocks
    // default: start in a not-in-conflict state
    let slMergeDirExists = false;
    let conflictData: ResolveCommandConflictOutput = NOT_IN_CONFLICT;

    /**
     * the next time repo.checkForMergeConflicts is called, this new conflict data will be used
     */
    function enterMergeConflict(conflict: ResolveCommandConflictOutput) {
      slMergeDirExists = true;
      conflictData = conflict;
    }

    beforeEach(() => {
      slMergeDirExists = false;
      conflictData = NOT_IN_CONFLICT;

      jest.spyOn(fsUtils, 'exists').mockImplementation(() => Promise.resolve(slMergeDirExists));

      jest.spyOn(execa, 'default').mockImplementation((async (
        _cmd: string,
        args: Array<string>,
        // eslint-disable-next-line require-await
      ) => {
        const argStr = args?.join(' ');
        if (argStr.startsWith('resolve --tool internal:dumpjson --all')) {
          return {stdout: JSON.stringify(conflictData)};
        }
        return {stdout: ''};
      }) as unknown as typeof execa.default);
    });

    it('checks for merge conflicts', async () => {
      const repo = new Repository(repoInfo, mockLogger);

      const onChange = jest.fn();
      repo.onChangeConflictState(onChange);

      await repo.checkForMergeConflicts();
      expect(onChange).toHaveBeenCalledTimes(0);

      enterMergeConflict(MOCK_CONFLICT);

      await repo.checkForMergeConflicts();

      expect(onChange).toHaveBeenCalledWith({state: 'loading'});
      expect(onChange).toHaveBeenCalledWith({
        state: 'loaded',
        command: 'rebase',
        toContinue: 'rebase --continue',
        toAbort: 'rebase --abort',
        files: [
          {path: 'file1.txt', status: 'U'},
          {path: 'file2.txt', status: 'U'},
        ],
      });
    });

    it('disposes conflict change subscriptions', async () => {
      const repo = new Repository(repoInfo, mockLogger);

      const onChange = jest.fn();
      const subscription = repo.onChangeConflictState(onChange);
      subscription.dispose();

      enterMergeConflict(MOCK_CONFLICT);
      await repo.checkForMergeConflicts();
      expect(onChange).toHaveBeenCalledTimes(0);
    });

    it('sends conflicts right away on subscription if already in conflicts', async () => {
      enterMergeConflict(MOCK_CONFLICT);

      const repo = new Repository(repoInfo, mockLogger);

      const onChange = jest.fn();
      repo.onChangeConflictState(onChange);
      await nextTick(); // allow message to get sent

      expect(onChange).toHaveBeenCalledWith({state: 'loading'});
      expect(onChange).toHaveBeenCalledWith({
        state: 'loaded',
        command: 'rebase',
        toContinue: 'rebase --continue',
        toAbort: 'rebase --abort',
        files: [
          {path: 'file1.txt', status: 'U'},
          {path: 'file2.txt', status: 'U'},
        ],
      });
    });

    it('preserves previous conflicts as resolved', async () => {
      const repo = new Repository(repoInfo, mockLogger);
      const onChange = jest.fn();
      repo.onChangeConflictState(onChange);

      enterMergeConflict(MOCK_CONFLICT);
      await repo.checkForMergeConflicts();
      expect(onChange).toHaveBeenCalledWith({
        state: 'loaded',
        command: 'rebase',
        toContinue: 'rebase --continue',
        toAbort: 'rebase --abort',
        files: [
          {path: 'file1.txt', status: 'U'},
          {path: 'file2.txt', status: 'U'},
        ],
      });

      enterMergeConflict(MOCK_CONFLICT_WITH_FILE1_RESOLVED);
      await repo.checkForMergeConflicts();
      expect(onChange).toHaveBeenCalledWith({
        state: 'loaded',
        command: 'rebase',
        toContinue: 'rebase --continue',
        toAbort: 'rebase --abort',
        files: [
          // even though file1 is no longer in the output, we remember it from before.
          {path: 'file1.txt', status: 'Resolved'},
          {path: 'file2.txt', status: 'U'},
        ],
      });
    });

    it('handles errors from `sl resolve`', async () => {
      jest.spyOn(execa, 'default').mockImplementation((async (
        _cmd: string,
        args: Array<string>,
        // eslint-disable-next-line require-await
      ) => {
        const argStr = args?.join(' ');
        if (argStr.startsWith('resolve --tool internal:dumpjson --all')) {
          throw new Error('failed to do the thing');
        }
        return {stdout: ''};
      }) as unknown as typeof execa.default);

      const repo = new Repository(repoInfo, mockLogger);
      const onChange = jest.fn();
      repo.onChangeConflictState(onChange);

      enterMergeConflict(MOCK_CONFLICT);
      await expect(repo.checkForMergeConflicts()).resolves.toEqual(undefined);

      expect(onChange).toHaveBeenCalledWith({state: 'loading'});
      expect(onChange).toHaveBeenCalledWith(undefined);
    });
  });
});

describe('extractGithubRepoInfoFromUrl', () => {
  it('handles http', () => {
    expect(extractGithubRepoInfoFromUrl('https://github.com/myUsername/myRepo.git')).toEqual({
      owner: 'myUsername',
      repo: 'myRepo',
    });
  });
  it('handles git@github', () => {
    expect(extractGithubRepoInfoFromUrl('git@github.com:myUsername/myRepo.git')).toEqual({
      owner: 'myUsername',
      repo: 'myRepo',
    });
  });
  it('handles ssh', () => {
    expect(extractGithubRepoInfoFromUrl('ssh://git@github.com:myUsername/myRepo.git')).toEqual({
      owner: 'myUsername',
      repo: 'myRepo',
    });
  });
  it('handles git+ssh', () => {
    expect(extractGithubRepoInfoFromUrl('git+ssh://git@github.com:myUsername/myRepo.git')).toEqual({
      owner: 'myUsername',
      repo: 'myRepo',
    });
  });
  it('handles dotted http', () => {
    expect(
      extractGithubRepoInfoFromUrl('https://github.com/myUsername/my.dotted.repo.git'),
    ).toEqual({owner: 'myUsername', repo: 'my.dotted.repo'});
  });
  it('handles dotted ssh', () => {
    expect(extractGithubRepoInfoFromUrl('git@github.com:myUsername/my.dotted.repo.git')).toEqual({
      owner: 'myUsername',
      repo: 'my.dotted.repo',
    });
  });
});
