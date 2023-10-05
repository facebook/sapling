/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ServerSideTracker} from '../analytics/serverSideTracker';
import type {FullTrackData} from '../analytics/types';
import type {ServerPlatform} from '../serverPlatform';

import {Repository} from '../Repository';
import {makeServerSideTracker} from '../analytics/serverSideTracker';
import * as execa from 'execa';
import {mockLogger} from 'shared/testUtils';
import {defer} from 'shared/utils';

/** Matches any non-empty string */
const anyActualString = expect.stringMatching(/.+/);

const mockTracker = makeServerSideTracker(
  mockLogger,
  {platformName: 'test'} as ServerPlatform,
  '0.1',
  jest.fn(),
);

jest.mock('../WatchForChanges', () => {
  class MockWatchForChanges {
    dispose = jest.fn();
  }
  return {WatchForChanges: MockWatchForChanges};
});

jest.mock('execa', () => {
  return jest.fn();
});

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

describe('track', () => {
  const mockSendData = jest.fn();
  let tracker: ServerSideTracker;

  beforeEach(() => {
    mockSendData.mockClear();
    tracker = makeServerSideTracker(
      mockLogger,
      {platformName: 'test'} as ServerPlatform,
      '0.1',
      mockSendData,
    );
  });
  it('tracks events', () => {
    tracker.track('ClickedRefresh');
    expect(mockSendData).toHaveBeenCalledWith(
      expect.objectContaining({eventName: 'ClickedRefresh'}),
      mockLogger,
    );
  });

  it('defines all fields', () => {
    tracker.track('ClickedRefresh');
    expect(mockSendData).toHaveBeenCalledWith(
      {
        eventName: anyActualString,
        timestamp: expect.anything(),
        id: expect.anything(),

        platform: 'test',
        version: '0.1',
        sessionId: anyActualString,
        unixname: anyActualString,
        repo: undefined,
        osType: anyActualString,
        osArch: anyActualString,
        osRelease: anyActualString,
        hostname: anyActualString,
      },
      mockLogger,
    );
  });

  it('allows setting repository', () => {
    // No need to call the actual command lines to test tracking
    const execaSpy = mockExeca([
      [/^sl config paths.default/, {stdout: 'https://github.com/facebook/sapling.git'}],
      [/^sl config github.pull_request_domain/, {stdout: 'github.com'}],
      [/^sl root --dotdir/, {stdout: '/path/to/myRepo/.sl'}],
      [/^sl root/, {stdout: '/path/to/myRepo'}],
      [
        /^gh auth status --hostname gitlab.myCompany.com/,
        new Error('not authenticated on this hostname'),
      ],
      [/^gh auth status --hostname ghe.myCompany.com/, {stdout: ''}],
      [/^gh api graphql/, {stdout: '{}'}],
    ]);

    const repo = new Repository(
      {
        type: 'success',
        codeReviewSystem: {
          type: 'github',
          repo: 'sapling',
          owner: 'facebook',
          hostname: 'github.com',
        },
        command: 'sl',
        repoRoot: '/path',
        dotdir: '/path/.sl',
        pullRequestDomain: undefined,
      },
      mockLogger,
      mockTracker,
    );
    tracker.context.setRepo(repo);
    tracker.track('ClickedRefresh');
    expect(mockSendData).toHaveBeenCalledWith(
      expect.objectContaining({
        repo: 'github:github.com/facebook/sapling',
      }),
      mockLogger,
    );
    repo.dispose();
    execaSpy.mockClear();
  });

  it('uses consistent session id, but different track ids', () => {
    tracker.track('ClickedRefresh');
    tracker.track('ClickedRefresh');
    const call0 = mockSendData.mock.calls[0][0] as FullTrackData;
    const call1 = mockSendData.mock.calls[1][0] as FullTrackData;
    expect(call0.id).not.toEqual(call1.id);
    expect(call0.sessionId).toEqual(call1.sessionId);
  });

  it('supports trees of events via trackAsParent', () => {
    const childTracker = tracker.trackAsParent('ClickedRefresh');
    childTracker.track('ClickedRefresh');
    const call0 = mockSendData.mock.calls[0][0];
    const call1 = mockSendData.mock.calls[1][0];
    expect(call0.id).toEqual(call1.parentId);
  });

  describe('trackError', () => {
    it('handles synchronous operations throwing', () => {
      tracker.error('ClickedRefresh', 'RepositoryError', new Error('uh oh'), {});
      expect(mockSendData).toHaveBeenCalledWith(
        expect.objectContaining({
          eventName: 'ClickedRefresh',
          errorName: 'RepositoryError',
          errorMessage: 'uh oh',
        }),
        mockLogger,
      );
    });
  });

  describe('trackOperation', () => {
    it('handles synchronous operations', () => {
      const op = jest.fn();
      tracker.operation('ClickedRefresh', 'RepositoryError', {}, op);
      expect(mockSendData).toHaveBeenCalledWith(
        expect.objectContaining({
          eventName: 'ClickedRefresh',
        }),
        mockLogger,
      );
      // there should not be an error field filled out
      expect(mockSendData).not.toHaveBeenCalledWith(
        expect.objectContaining({
          errorName: anyActualString,
          errorMessage: anyActualString,
        }),
        mockLogger,
      );
      expect(op).toHaveBeenCalled();
    });

    it('handles synchronous operations throwing', () => {
      const op = jest.fn().mockImplementation(() => {
        throw new Error('uh oh');
      });
      expect(() => tracker.operation('ClickedRefresh', 'RepositoryError', {}, op)).toThrow();
      expect(mockSendData).toHaveBeenCalledWith(
        expect.objectContaining({
          eventName: 'ClickedRefresh',
          errorName: 'RepositoryError',
          errorMessage: 'uh oh',
        }),
        mockLogger,
      );
      expect(op).toHaveBeenCalled();
    });

    it('handles async operations', async () => {
      const d = defer();
      const op = jest.fn().mockImplementation(() => {
        return d.promise;
      });

      const promise = tracker.operation('ClickedRefresh', 'RepositoryError', {}, op);
      expect(mockSendData).not.toHaveBeenCalled();

      d.resolve(1);

      await promise;

      expect(mockSendData).toHaveBeenCalledWith(
        expect.objectContaining({
          eventName: 'ClickedRefresh',
        }),
        mockLogger,
      );
      // there should not be an error field filled out
      expect(mockSendData).not.toHaveBeenCalledWith(
        expect.objectContaining({
          errorName: anyActualString,
          errorMessage: anyActualString,
        }),
        mockLogger,
      );
      expect(op).toHaveBeenCalled();
    });

    it('handles async operations throwing', async () => {
      const d = defer();
      const op = jest.fn().mockImplementation(() => {
        return d.promise;
      });

      const promise = tracker.operation('ClickedRefresh', 'RepositoryError', {}, op);
      expect(mockSendData).not.toHaveBeenCalled();

      d.reject(new Error('oh no'));

      await expect(promise).rejects.toEqual(new Error('oh no'));

      expect(mockSendData).toHaveBeenCalledWith(
        expect.objectContaining({
          eventName: 'ClickedRefresh',
          errorName: 'RepositoryError',
          errorMessage: 'oh no',
        }),
        mockLogger,
      );
      expect(op).toHaveBeenCalled();
    });
  });
});
