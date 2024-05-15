/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CodeReviewSystem} from '../types';
import type {DateTuple} from 'shared/types/common';

import App from '../App';
import {
  resetTestMessages,
  expectMessageSentToServer,
  simulateCommits,
  closeCommitInfoSidebar,
  TEST_COMMIT_HISTORY,
  simulateMessageFromServer,
  expectMessageNOTSentToServer,
} from '../testUtils';
import {CommandRunner} from '../types';
import {fireEvent, render, screen, act, waitFor, within} from '@testing-library/react';
import * as utils from 'shared/utils';

const EXPORT_STACK_DATA = [
  {
    requested: false,
    node: 'd',
    author: 'username',
    date: [1715719789, 25200] as DateTuple,
    text: 'Commit D',
    immutable: false,
    relevantFiles: {
      'myFile.js': {
        data: 'hello\nworld!\n',
      },
    },
  },
  {
    requested: true,
    node: 'e',
    author: 'username',
    date: [1715719789, 25200] as DateTuple,
    text: 'Commit E',
    immutable: false,
    parents: ['d'],
    files: {
      'myFile.js': {
        data: 'hello (changed)\nworld!\n',
      },
    },
  },
];

describe('Interactive Split', () => {
  beforeEach(() => {
    resetTestMessages();
    render(<App />);
    act(() => {
      closeCommitInfoSidebar();
      simulateMessageFromServer({
        type: 'repoInfo',
        info: {
          type: 'success',
          repoRoot: '/path/to/repo',
          dotdir: '/path/to/repo/.sl',
          command: 'sl',
          pullRequestDomain: undefined,
          codeReviewSystem: {type: 'github'} as CodeReviewSystem,
        },
      });
      expectMessageSentToServer({
        type: 'subscribe',
        kind: 'smartlogCommits',
        subscriptionID: expect.anything(),
      });
      simulateCommits({
        value: TEST_COMMIT_HISTORY,
      });
    });

    const mockObserveFn = () => {
      return {
        observe: jest.fn(),
        unobserve: jest.fn(),
        disconnect: jest.fn(),
      };
    };

    window.IntersectionObserver = jest.fn().mockImplementation(mockObserveFn);
  });

  const mockNextOperationId = (id: string) =>
    jest.spyOn(utils, 'randomId').mockImplementationOnce(() => id);

  it('shows split button on dot commit', () => {
    expect(screen.getByText('Split')).toBeInTheDocument();
  });

  it('show split modal with spinner on click', async () => {
    fireEvent.click(screen.getByText('Split'));
    await waitFor(() => expect(screen.getByTestId('edit-stack-loading')).toBeInTheDocument());
  });

  it('requests debugexportstack data', async () => {
    fireEvent.click(screen.getByText('Split'));

    await waitFor(() => expectMessageSentToServer({type: 'exportStack', revs: 'e'}));
  });

  it('shows errors', async () => {
    fireEvent.click(screen.getByText('Split'));
    await waitFor(() => expectMessageSentToServer({type: 'exportStack', revs: 'e'}));
    act(() => {
      simulateMessageFromServer({
        type: 'exportedStack',
        revs: 'e',
        assumeTracked: [],
        error: 'test error',
        stack: [],
      });
    });
    await waitFor(() => expect(screen.getByText('test error')).toBeInTheDocument());
  });

  it('waits for existing commands to finish running before loading stack', async () => {
    mockNextOperationId('1');
    fireEvent.click(screen.getByText('Pull', {selector: 'button'}));
    fireEvent.click(screen.getByText('Split'));

    expectMessageNOTSentToServer({type: 'exportStack', revs: 'e'});

    act(() =>
      simulateMessageFromServer({
        type: 'operationProgress',
        id: '1',
        kind: 'exit',
        exitCode: 0,
        timestamp: 0,
      }),
    );

    await waitFor(() => expectMessageSentToServer({type: 'exportStack', revs: 'e'}));
  });

  describe('with loaded stack data', () => {
    beforeEach(async () => {
      fireEvent.click(screen.getByText('Split'));
      await waitFor(() => expectMessageSentToServer({type: 'exportStack', revs: 'e'}));
      act(() => {
        simulateMessageFromServer({
          type: 'exportedStack',
          revs: 'e',
          assumeTracked: [],
          error: undefined,
          stack: EXPORT_STACK_DATA,
        });
      });
      await waitFor(() =>
        expect(screen.getByTestId('interactive-split-modal')).toBeInTheDocument(),
      );
    });

    it('loads exported stack into UI', () => {
      expect(
        within(screen.getByTestId('interactive-split-modal')).getByText('myFile.js'),
      ).toBeInTheDocument();

      expect(
        within(screen.getByTestId('interactive-split-modal')).getByText('hello'),
      ).toBeInTheDocument();

      expect(
        within(screen.getByTestId('interactive-split-modal')).getByText('hello (changed)'),
      ).toBeInTheDocument();
    });

    it('moves lines and requests importing', async () => {
      jest.useFakeTimers().setSystemTime(new Date('2020-01-01'));

      const arrows = screen.getAllByTitle('Move this line change right');
      fireEvent.click(arrows[1]);
      mockNextOperationId('2');
      fireEvent.click(screen.getByTestId('confirm-edit-stack-button'));

      await waitFor(() =>
        expectMessageSentToServer({
          type: 'runOperation',
          operation: {
            id: '2',
            trackEventName: 'ImportStackOperation',
            args: ['debugimportstack'],
            stdin: JSON.stringify([
              [
                'commit',
                {
                  mark: ':r1',
                  author: 'username',
                  date: [1577836800, 25200],
                  text: 'Commit E',
                  parents: ['d'],
                  predecessors: ['e'],
                  files: {'myFile.js': {data: 'world!\n', flags: ''}},
                },
              ],
              [
                'commit',
                {
                  mark: ':r2',
                  author: 'username',
                  date: [1577836800, 25200],
                  text: 'Split of "Commit E"',
                  parents: [':r1'],
                  predecessors: ['e'],
                  files: {'myFile.js': {data: 'hello (changed)\nworld!\n', flags: ''}},
                },
              ],
              ['goto', {mark: ':r2'}],
            ]),
            runner: CommandRunner.Sapling,
          },
        }),
      );
    });
  });
});
