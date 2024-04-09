/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ResolveCommandConflictOutput} from '../commands';
import type {ServerPlatform} from '../serverPlatform';
import type {RepositoryContext} from '../serverTypes';
import type {RunnableOperation} from 'isl/src/types';

import {absolutePathForFileInRepo, Repository} from '../Repository';
import {makeServerSideTracker} from '../analytics/serverSideTracker';
import {extractRepoInfoFromUrl, setConfigOverrideForTests} from '../commands';
import * as execa from 'execa';
import {CommandRunner, type MergeConflicts, type ValidatedRepoInfo} from 'isl/src/types';
import os from 'os';
import path from 'path';
import * as fsUtils from 'shared/fs';
import {clone, mockLogger, nextTick} from 'shared/testUtils';

/* eslint-disable require-await */

jest.mock('execa', () => {
  return jest.fn();
});

jest.mock('../WatchForChanges', () => {
  class MockWatchForChanges {
    dispose = jest.fn();
    poll = jest.fn();
  }
  return {WatchForChanges: MockWatchForChanges};
});

const mockTracker = makeServerSideTracker(
  mockLogger,
  {platformName: 'test'} as ServerPlatform,
  '0.1',
  jest.fn(),
);

function mockExeca(
  cmds: Array<[RegExp, (() => {stdout: string} | Error) | {stdout: string} | Error]>,
) {
  return jest.spyOn(execa, 'default').mockImplementation(((cmd: string, args: Array<string>) => {
    const argStr = cmd + ' ' + args?.join(' ');
    const execaOther = {
      kill: jest.fn(),
      on: jest.fn((event, cb) => {
        // immediately call exit cb to teardown timeout
        if (event === 'exit') {
          cb();
        }
      }),
    };
    for (const [regex, output] of cmds) {
      if (regex.test(argStr)) {
        let value = output;
        if (typeof output === 'function') {
          value = output();
        }
        if (value instanceof Error) {
          throw value;
        }
        return {...execaOther, ...value};
      }
    }
    return {...execaOther, stdout: ''};
  }) as unknown as typeof execa.default);
}

function processExitError(code: number, message: string): execa.ExecaError {
  const err = new Error(message) as execa.ExecaError;
  err.exitCode = code;
  return err;
}

function setPathsDefault(path: string) {
  setConfigOverrideForTests([['paths.default', path]], false);
}

describe('Repository', () => {
  let ctx: RepositoryContext;
  beforeEach(() => {
    ctx = {
      cmd: 'sl',
      cwd: '/path/to/cwd',
      logger: mockLogger,
      tracker: mockTracker,
    };
  });

  it('setting command name', async () => {
    const execaSpy = mockExeca([]);
    await Repository.getRepoInfo({...ctx, cmd: 'slb'});
    expect(execaSpy).toHaveBeenCalledWith(
      'slb',
      expect.arrayContaining(['root']),
      expect.anything(),
    );
  });

  describe('extracting github repo info', () => {
    beforeEach(() => {
      setConfigOverrideForTests([['github.pull_request_domain', 'github.com']]);
      mockExeca([
        [/^sl root --dotdir/, {stdout: '/path/to/myRepo/.sl'}],
        [/^sl root/, {stdout: '/path/to/myRepo'}],
        [
          /^gh auth status --hostname gitlab.myCompany.com/,
          new Error('not authenticated on this hostname'),
        ],
        [/^gh auth status --hostname ghe.myCompany.com/, {stdout: ''}],
      ]);
    });

    it('extracting github repo info', async () => {
      setPathsDefault('https://github.com/myUsername/myRepo.git');
      const info = (await Repository.getRepoInfo(ctx)) as ValidatedRepoInfo;
      const repo = new Repository(info, ctx);
      expect(repo.info).toEqual({
        type: 'success',
        command: 'sl',
        repoRoot: '/path/to/myRepo',
        dotdir: '/path/to/myRepo/.sl',
        codeReviewSystem: {
          type: 'github',
          owner: 'myUsername',
          repo: 'myRepo',
          hostname: 'github.com',
        },
        pullRequestDomain: 'github.com',
      });
    });

    it('extracting github enterprise repo info', async () => {
      setPathsDefault('https://ghe.myCompany.com/myUsername/myRepo.git');
      const info = (await Repository.getRepoInfo(ctx)) as ValidatedRepoInfo;
      const repo = new Repository(info, ctx);
      expect(repo.info).toEqual({
        type: 'success',
        command: 'sl',
        repoRoot: '/path/to/myRepo',
        dotdir: '/path/to/myRepo/.sl',
        codeReviewSystem: {
          type: 'github',
          owner: 'myUsername',
          repo: 'myRepo',
          hostname: 'ghe.myCompany.com',
        },
        pullRequestDomain: 'github.com',
      });
    });

    it('handles non-github-enterprise unknown code review providers', async () => {
      setPathsDefault('https://gitlab.myCompany.com/myUsername/myRepo.git');
      const info = (await Repository.getRepoInfo(ctx)) as ValidatedRepoInfo;
      const repo = new Repository(info, ctx);
      expect(repo.info).toEqual({
        type: 'success',
        command: 'sl',
        repoRoot: '/path/to/myRepo',
        dotdir: '/path/to/myRepo/.sl',
        codeReviewSystem: {
          type: 'unknown',
          path: 'https://gitlab.myCompany.com/myUsername/myRepo.git',
        },
        pullRequestDomain: 'github.com',
      });
    });
  });

  it('applies isl.hold-off-refresh-ms config', async () => {
    setConfigOverrideForTests([['isl.hold-off-refresh-ms', '12345']], false);
    const info = (await Repository.getRepoInfo(ctx)) as ValidatedRepoInfo;
    const repo = new Repository(info, ctx);
    await new Promise(process.nextTick);
    expect(repo.configHoldOffRefreshMs).toBe(12345);
  });

  it('extracting repo info', async () => {
    setConfigOverrideForTests([]);
    setPathsDefault('mononoke://0.0.0.0/fbsource');
    mockExeca([
      [/^sl root --dotdir/, {stdout: '/path/to/myRepo/.sl'}],
      [/^sl root/, {stdout: '/path/to/myRepo'}],
    ]);
    const info = (await Repository.getRepoInfo(ctx)) as ValidatedRepoInfo;
    const repo = new Repository(info, ctx);
    expect(repo.info).toEqual({
      type: 'success',
      command: 'sl',
      repoRoot: '/path/to/myRepo',
      dotdir: '/path/to/myRepo/.sl',
      codeReviewSystem: expect.anything(),
      pullRequestDomain: undefined,
    });
  });

  it('handles cwd not exists', async () => {
    const err = new Error('cwd does not exist') as Error & {code: string};
    err.code = 'ENOENT';
    mockExeca([[/^sl root/, err]]);
    const info = (await Repository.getRepoInfo(ctx)) as ValidatedRepoInfo;
    expect(info).toEqual({
      type: 'cwdDoesNotExist',
      cwd: '/path/to/cwd',
    });
  });

  it('handles missing executables on windows', async () => {
    const osSpy = jest.spyOn(os, 'platform').mockImplementation(() => 'win32');
    mockExeca([
      [
        /^sl root/,
        processExitError(
          /* code */ 1,
          `'sl' is not recognized as an internal or external command, operable program or batch file.`,
        ),
      ],
    ]);
    jest.spyOn(fsUtils, 'exists').mockImplementation(async () => true);
    const info = (await Repository.getRepoInfo(ctx)) as ValidatedRepoInfo;
    expect(info).toEqual({
      type: 'invalidCommand',
      command: 'sl',
      path: expect.anything(),
    });
    osSpy.mockRestore();
  });

  it('prevents setting configs not in the allowlist', async () => {
    setConfigOverrideForTests([]);
    setPathsDefault('mononoke://0.0.0.0/fbsource');
    mockExeca([
      [/^sl root --dotdir/, {stdout: '/path/to/myRepo/.sl'}],
      [/^sl root/, {stdout: '/path/to/myRepo'}],
    ]);
    const info = (await Repository.getRepoInfo(ctx)) as ValidatedRepoInfo;
    const repo = new Repository(info, ctx);
    // @ts-expect-error We expect a type error in addition to runtime validation
    await expect(repo.setConfig(ctx, 'user', 'some-random-config', 'hi')).rejects.toEqual(
      new Error('config some-random-config not in allowlist for settable configs'),
    );
  });

  describe('running operations', () => {
    const repoInfo: ValidatedRepoInfo = {
      type: 'success',
      command: 'sl',
      dotdir: '/path/to/repo/.sl',
      repoRoot: '/path/to/repo',
      codeReviewSystem: {type: 'unknown'},
      pullRequestDomain: undefined,
    };

    let execaSpy: ReturnType<typeof mockExeca>;
    beforeEach(() => {
      execaSpy = mockExeca([]);
    });

    async function runOperation(op: Partial<RunnableOperation>) {
      const repo = new Repository(repoInfo, ctx);
      const progressSpy = jest.fn();

      await repo.runOrQueueOperation(
        ctx,
        {
          id: '1',
          trackEventName: 'CommitOperation',
          args: [],
          runner: CommandRunner.Sapling,
          ...op,
        },
        progressSpy,
      );
    }

    it('runs operations', async () => {
      runOperation({
        args: ['commit', '--message', 'hi'],
      });

      expect(execaSpy).toHaveBeenCalledWith(
        'sl',
        ['commit', '--message', 'hi', '--noninteractive'],
        expect.anything(),
      );
    });

    it('handles succeedable revsets', async () => {
      runOperation({
        args: ['rebase', '--rev', {type: 'succeedable-revset', revset: 'aaa'}],
      });

      expect(execaSpy).toHaveBeenCalledWith(
        'sl',
        ['rebase', '--rev', 'max(successors(aaa))', '--noninteractive'],
        expect.anything(),
      );
    });

    it('handles exact revsets', async () => {
      runOperation({
        args: ['rebase', '--rev', {type: 'exact-revset', revset: 'aaa'}],
      });

      expect(execaSpy).toHaveBeenCalledWith(
        'sl',
        ['rebase', '--rev', 'aaa', '--noninteractive'],
        expect.anything(),
      );
    });

    it('handles repo-relative files', async () => {
      runOperation({
        args: ['add', {type: 'repo-relative-file', path: 'path/to/file.txt'}],
      });

      expect(execaSpy).toHaveBeenCalledWith(
        'sl',
        ['add', '../repo/path/to/file.txt', '--noninteractive'],
        expect.anything(),
      );
    });

    it('handles allowed configs', async () => {
      runOperation({
        args: ['commit', {type: 'config', key: 'ui.allowemptycommit', value: 'True'}],
      });

      expect(execaSpy).toHaveBeenCalledWith(
        'sl',
        ['commit', '--config', 'ui.allowemptycommit=True', '--noninteractive'],
        expect.anything(),
      );
    });

    it('disallows some commands', async () => {
      runOperation({
        args: ['debugsh'],
      });

      expect(execaSpy).not.toHaveBeenCalledWith(
        'sl',
        ['debugsh', '--noninteractive'],
        expect.anything(),
      );
    });

    it('disallows unknown configs', async () => {
      runOperation({
        args: ['commit', {type: 'config', key: 'foo.bar', value: '1'}],
      });

      expect(execaSpy).not.toHaveBeenCalledWith(
        'sl',
        expect.arrayContaining(['commit', '--config', 'foo.bar=1']),
        expect.anything(),
      );
    });

    it('disallows unstructured --config flag', async () => {
      runOperation({
        args: ['commit', '--config', 'foo.bar=1'],
      });

      expect(execaSpy).not.toHaveBeenCalledWith(
        'sl',
        expect.arrayContaining(['commit', '--config', 'foo.bar=1']),
        expect.anything(),
      );
    });
  });

  describe('fetchSmartlogCommits', () => {
    const repoInfo: ValidatedRepoInfo = {
      type: 'success',
      command: 'sl',
      dotdir: '/path/to/repo/.sl',
      repoRoot: '/path/to/repo',
      codeReviewSystem: {type: 'unknown'},
      pullRequestDomain: undefined,
    };

    const expectCalledWithRevset = (spy: jest.SpyInstance<unknown>, revset: string) => {
      expect(spy).toHaveBeenCalledWith(
        'sl',
        expect.arrayContaining(['log', '--rev', revset]),
        expect.anything(),
      );
    };

    it('uses correct revset in normal case', async () => {
      const repo = new Repository(repoInfo, ctx);

      const execaSpy = mockExeca([]);

      await repo.fetchSmartlogCommits();
      expectCalledWithRevset(
        execaSpy,
        'smartlog(((interestingbookmarks() + heads(draft())) & date(-14)) + .)',
      );
    });

    it('updates revset when changing date range', async () => {
      const execaSpy = mockExeca([]);
      const repo = new Repository(repoInfo, ctx);

      repo.nextVisibleCommitRangeInDays();
      await repo.fetchSmartlogCommits();
      expectCalledWithRevset(
        execaSpy,
        'smartlog(((interestingbookmarks() + heads(draft())) & date(-60)) + .)',
      );

      repo.nextVisibleCommitRangeInDays();
      await repo.fetchSmartlogCommits();
      expectCalledWithRevset(execaSpy, 'smartlog((interestingbookmarks() + heads(draft())) + .)');
    });

    it('fetches additional revsets', async () => {
      const execaSpy = mockExeca([]);
      const repo = new Repository(repoInfo, ctx);

      repo.stableLocations = [
        {name: 'mystable', hash: 'aaa', info: 'this is the stable for aaa', date: new Date(0)},
      ];
      await repo.fetchSmartlogCommits();
      expectCalledWithRevset(
        execaSpy,
        'smartlog(((interestingbookmarks() + heads(draft())) & date(-14)) + . + present(aaa))',
      );

      repo.stableLocations = [
        {name: 'mystable', hash: 'aaa', info: 'this is the stable for aaa', date: new Date(0)},
        {name: '2', hash: 'bbb', info: '2', date: new Date(0)},
      ];
      await repo.fetchSmartlogCommits();
      expectCalledWithRevset(
        execaSpy,
        'smartlog(((interestingbookmarks() + heads(draft())) & date(-14)) + . + present(aaa) + present(bbb))',
      );

      repo.nextVisibleCommitRangeInDays();
      repo.nextVisibleCommitRangeInDays();
      await repo.fetchSmartlogCommits();
      expectCalledWithRevset(
        execaSpy,
        'smartlog((interestingbookmarks() + heads(draft())) + . + present(aaa) + present(bbb))',
      );
    });
  });

  describe('merge conflicts', () => {
    const repoInfo: ValidatedRepoInfo = {
      type: 'success',
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

      mockExeca([
        [
          /^sl resolve --tool internal:dumpjson --all/,
          () => ({stdout: JSON.stringify(conflictData)}),
        ],
      ]);
    });

    it('checks for merge conflicts', async () => {
      const repo = new Repository(repoInfo, ctx);

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
        fetchStartTimestamp: expect.anything(),
        fetchCompletedTimestamp: expect.anything(),
      } as MergeConflicts);
    });

    it('disposes conflict change subscriptions', async () => {
      const repo = new Repository(repoInfo, ctx);

      const onChange = jest.fn();
      const subscription = repo.onChangeConflictState(onChange);
      subscription.dispose();

      enterMergeConflict(MOCK_CONFLICT);
      await repo.checkForMergeConflicts();
      expect(onChange).toHaveBeenCalledTimes(0);
    });

    it('sends conflicts right away on subscription if already in conflicts', async () => {
      enterMergeConflict(MOCK_CONFLICT);

      const repo = new Repository(repoInfo, ctx);

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
        fetchStartTimestamp: expect.anything(),
        fetchCompletedTimestamp: expect.anything(),
      });
    });

    it('preserves previous conflicts as resolved', async () => {
      const repo = new Repository(repoInfo, ctx);
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
        fetchStartTimestamp: expect.anything(),
        fetchCompletedTimestamp: expect.anything(),
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
        fetchStartTimestamp: expect.anything(),
        fetchCompletedTimestamp: expect.anything(),
      });
    });

    it('handles errors from `sl resolve`', async () => {
      mockExeca([
        [/^sl resolve --tool internal:dumpjson --all/, new Error('failed to do the thing')],
      ]);

      const repo = new Repository(repoInfo, ctx);
      const onChange = jest.fn();
      repo.onChangeConflictState(onChange);

      enterMergeConflict(MOCK_CONFLICT);
      await expect(repo.checkForMergeConflicts()).resolves.toEqual(undefined);

      expect(onChange).toHaveBeenCalledWith({state: 'loading'});
      expect(onChange).toHaveBeenCalledWith(undefined);
    });
  });
});

describe('extractRepoInfoFromUrl', () => {
  describe('github.com', () => {
    it('handles http', () => {
      expect(extractRepoInfoFromUrl('https://github.com/myUsername/myRepo.git')).toEqual({
        owner: 'myUsername',
        repo: 'myRepo',
        hostname: 'github.com',
      });
    });
    it('handles plain github.com', () => {
      expect(extractRepoInfoFromUrl('github.com/myUsername/myRepo.git')).toEqual({
        owner: 'myUsername',
        repo: 'myRepo',
        hostname: 'github.com',
      });
    });
    it('handles git@github', () => {
      expect(extractRepoInfoFromUrl('git@github.com:myUsername/myRepo.git')).toEqual({
        owner: 'myUsername',
        repo: 'myRepo',
        hostname: 'github.com',
      });
    });
    it('handles ssh with slashes', () => {
      expect(extractRepoInfoFromUrl('ssh://git@github.com/myUsername/my-repo.git')).toEqual({
        owner: 'myUsername',
        repo: 'my-repo',
        hostname: 'github.com',
      });
    });
    it('handles git+ssh', () => {
      expect(extractRepoInfoFromUrl('git+ssh://git@github.com:myUsername/myRepo.git')).toEqual({
        owner: 'myUsername',
        repo: 'myRepo',
        hostname: 'github.com',
      });
    });
    it('handles dotted http', () => {
      expect(extractRepoInfoFromUrl('https://github.com/myUsername/my.dotted.repo.git')).toEqual({
        owner: 'myUsername',
        repo: 'my.dotted.repo',
        hostname: 'github.com',
      });
    });
    it('handles dotted ssh', () => {
      expect(extractRepoInfoFromUrl('git@github.com:myUsername/my.dotted.repo.git')).toEqual({
        owner: 'myUsername',
        repo: 'my.dotted.repo',
        hostname: 'github.com',
      });
    });
  });

  describe('github enterprise', () => {
    it('handles http', () => {
      expect(extractRepoInfoFromUrl('https://ghe.company.com/myUsername/myRepo.git')).toEqual({
        owner: 'myUsername',
        repo: 'myRepo',
        hostname: 'ghe.company.com',
      });
    });
    it('handles plain github.com', () => {
      expect(extractRepoInfoFromUrl('ghe.company.com/myUsername/myRepo.git')).toEqual({
        owner: 'myUsername',
        repo: 'myRepo',
        hostname: 'ghe.company.com',
      });
    });
    it('handles git@github', () => {
      expect(extractRepoInfoFromUrl('git@ghe.company.com:myUsername/myRepo.git')).toEqual({
        owner: 'myUsername',
        repo: 'myRepo',
        hostname: 'ghe.company.com',
      });
    });
    it('handles ssh with slashes', () => {
      expect(extractRepoInfoFromUrl('ssh://git@ghe.company.com/myUsername/my-repo.git')).toEqual({
        owner: 'myUsername',
        repo: 'my-repo',
        hostname: 'ghe.company.com',
      });
    });
    it('handles git+ssh', () => {
      expect(extractRepoInfoFromUrl('git+ssh://git@ghe.company.com:myUsername/myRepo.git')).toEqual(
        {
          owner: 'myUsername',
          repo: 'myRepo',
          hostname: 'ghe.company.com',
        },
      );
    });
    it('handles dotted http', () => {
      expect(
        extractRepoInfoFromUrl('https://ghe.company.com/myUsername/my.dotted.repo.git'),
      ).toEqual({
        owner: 'myUsername',
        repo: 'my.dotted.repo',
        hostname: 'ghe.company.com',
      });
    });
    it('handles dotted ssh', () => {
      expect(extractRepoInfoFromUrl('git@ghe.company.com:myUsername/my.dotted.repo.git')).toEqual({
        owner: 'myUsername',
        repo: 'my.dotted.repo',
        hostname: 'ghe.company.com',
      });
    });
  });
});

describe('absolutePathForFileInRepo', () => {
  let ctx: RepositoryContext;
  beforeEach(() => {
    ctx = {
      cmd: 'sl',
      cwd: '/path/to/cwd',
      logger: mockLogger,
      tracker: mockTracker,
    };
  });

  it('rejects .. in paths that escape the repo', () => {
    const repoInfo: ValidatedRepoInfo = {
      type: 'success',
      command: 'sl',
      dotdir: '/path/to/repo/.sl',
      repoRoot: '/path/to/repo',
      codeReviewSystem: {type: 'unknown'},
      pullRequestDomain: undefined,
    };
    const repo = new Repository(repoInfo, ctx);

    expect(absolutePathForFileInRepo('foo/bar/file.txt', repo)).toEqual(
      '/path/to/repo/foo/bar/file.txt',
    );
    expect(absolutePathForFileInRepo('foo/../bar/file.txt', repo)).toEqual(
      '/path/to/repo/bar/file.txt',
    );
    expect(absolutePathForFileInRepo('file.txt', repo)).toEqual('/path/to/repo/file.txt');

    expect(absolutePathForFileInRepo('/file.txt', repo)).toEqual(null);
    expect(absolutePathForFileInRepo('', repo)).toEqual(null);
    expect(absolutePathForFileInRepo('foo/../../file.txt', repo)).toEqual(null);
    expect(absolutePathForFileInRepo('../file.txt', repo)).toEqual(null);
    expect(absolutePathForFileInRepo('/../file.txt', repo)).toEqual(null);
  });

  it('works on windows', () => {
    const repoInfo: ValidatedRepoInfo = {
      type: 'success',
      command: 'sl',
      dotdir: 'C:\\path\\to\\repo\\.sl',
      repoRoot: 'C:\\path\\to\\repo',
      codeReviewSystem: {type: 'unknown'},
      pullRequestDomain: undefined,
    };
    const repo = new Repository(repoInfo, ctx);

    expect(absolutePathForFileInRepo('foo\\bar\\file.txt', repo, path.win32)).toEqual(
      'C:\\path\\to\\repo\\foo\\bar\\file.txt',
    );

    expect(absolutePathForFileInRepo('foo\\..\\..\\file.txt', repo, path.win32)).toEqual(null);
  });
});
