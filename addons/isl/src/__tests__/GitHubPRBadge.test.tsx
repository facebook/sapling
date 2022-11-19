/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DiffId, DiffSummary} from '../types';

import {PullRequestState} from '../../../isl-server/src/github/generated/graphql';
import App from '../App';
import platform from '../platform';
import {
  resetTestMessages,
  expectMessageSentToServer,
  simulateCommits,
  COMMIT,
  closeCommitInfoSidebar,
  simulateMessageFromServer,
  suppressReactErrorBoundaryErrorMessages,
} from '../testUtils';
import {fireEvent, render, screen, within} from '@testing-library/react';
import {act} from 'react-dom/test-utils';

jest.mock('../MessageBus');

describe('GitHubPRBadge', () => {
  beforeEach(() => {
    resetTestMessages();
    render(<App />);
    act(() => {
      closeCommitInfoSidebar();
      simulateCommits({
        value: [
          COMMIT('1', 'some public base', '0', {phase: 'public', diffId: '2'}),
          COMMIT('a', 'Commit A', '1', {diffId: '10'}),
          COMMIT('b', 'Commit B', 'a', {isHead: true, diffId: '11'}),
          COMMIT('c', 'Commit C', '1'),
        ],
      });
    });
  });

  it("doesn't render any github component when we don't know the remote repo provider", () => {
    expect(screen.queryAllByTestId('diff-spinner')).toHaveLength(0);
  });

  describe('in github repo', () => {
    beforeEach(() => {
      act(() => {
        simulateMessageFromServer({
          type: 'repoInfo',
          info: {
            type: 'success',
            command: 'sl',
            repoRoot: '/path/to/testrepo',
            dotdir: '/path/to/testrepo/.sl',
            codeReviewSystem: {
              type: 'github',
              repo: 'testrepo',
              owner: 'myusername',
              hostname: 'github.com',
            },
            pullRequestDomain: undefined,
            preferredSubmitCommand: 'pr',
          },
        });
      });
    });

    it('requests a diff fetch on mount', () => {
      expectMessageSentToServer({
        type: 'fetchDiffSummaries',
      });
    });

    it('renders spinners for commits with github PRs', () => {
      expect(screen.queryAllByTestId('diff-spinner')).toHaveLength(2);
    });

    describe('after PRs loaded', () => {
      beforeEach(() => {
        act(() => {
          simulateMessageFromServer({
            type: 'fetchedDiffSummaries',
            summaries: {
              value: new Map<DiffId, DiffSummary>([
                [
                  '10',
                  {
                    number: '10',
                    state: PullRequestState.Open,
                    title: 'PR A',
                    type: 'github',
                    url: 'https://github.com/myusername/testrepo/pull/10',
                    anyUnresolvedComments: false,
                    commentCount: 0,
                  },
                ],
                [
                  '11',
                  {
                    number: '11',
                    state: PullRequestState.Open,
                    title: 'PR B',
                    type: 'github',
                    url: 'https://github.com/myusername/testrepo/pull/11',
                    anyUnresolvedComments: false,
                    commentCount: 0,
                  },
                ],
              ]),
            },
          });
        });
      });

      it("doesn't show spinners anymore", () => {
        expect(screen.queryByTestId('diff-spinner')).not.toBeInTheDocument();
      });

      it('renders PR badges', () => {
        expect(screen.queryAllByTestId('github-diff-info')).toHaveLength(2);
        expect(
          within(
            within(screen.getByTestId('commit-a')).getByTestId('github-diff-info'),
          ).queryByText('#10'),
        ).toBeInTheDocument();
        expect(
          within(
            within(screen.getByTestId('commit-b')).getByTestId('github-diff-info'),
          ).queryByText('#11'),
        ).toBeInTheDocument();
      });

      it('does not render PR badge for public commits', () => {
        expect(
          within(screen.getByTestId('commit-1')).queryByTestId('github-diff-info'),
        ).not.toBeInTheDocument();
        expect(within(screen.getByTestId('commit-1')).queryByText('#2')).not.toBeInTheDocument();
      });

      describe('url opener', () => {
        beforeEach(() => {
          jest.spyOn(platform, 'openExternalLink').mockImplementation(() => undefined);
        });

        it('opens with default uri', () => {
          const prA = within(
            within(screen.getByTestId('commit-a')).getByTestId('github-diff-info'),
          ).getByText('open', {exact: false});
          fireEvent.click(prA);

          expect(platform.openExternalLink).toHaveBeenCalledWith(
            'https://github.com/myusername/testrepo/pull/10',
          );
        });

        it('uses custom opener', () => {
          act(() => {
            simulateMessageFromServer({
              type: 'repoInfo',
              info: {
                type: 'success',
                command: 'sl',
                repoRoot: '/path/to/testrepo',
                dotdir: '/path/to/testrepo/.sl',
                codeReviewSystem: {
                  type: 'github',
                  repo: 'testrepo',
                  owner: 'myusername',
                  hostname: 'github.com',
                },
                pullRequestDomain: 'https://myreviewsite.dev',
                preferredSubmitCommand: 'pr',
              },
            });
          });

          const prA = within(
            within(screen.getByTestId('commit-a')).getByTestId('github-diff-info'),
          ).getByText('open', {exact: false});
          fireEvent.click(prA);

          expect(platform.openExternalLink).toHaveBeenCalledWith(
            'https://myreviewsite.dev/myusername/testrepo/pull/10',
          );
        });

        it('uses custom opener with relaxed format', () => {
          act(() => {
            simulateMessageFromServer({
              type: 'repoInfo',
              info: {
                type: 'success',
                command: 'sl',
                repoRoot: '/path/to/testrepo',
                dotdir: '/path/to/testrepo/.sl',
                codeReviewSystem: {
                  type: 'github',
                  repo: 'testrepo',
                  owner: 'myusername',
                  hostname: 'github.com',
                },
                // no leading https://, adds custom prefix
                pullRequestDomain: 'myreviewsite.dev/codereview',
                preferredSubmitCommand: 'pr',
              },
            });
          });

          const prA = within(
            within(screen.getByTestId('commit-a')).getByTestId('github-diff-info'),
          ).getByText('open', {exact: false});
          fireEvent.click(prA);

          expect(platform.openExternalLink).toHaveBeenCalledWith(
            'https://myreviewsite.dev/codereview/myusername/testrepo/pull/10',
          );
        });
      });
    });

    describe('after error', () => {
      suppressReactErrorBoundaryErrorMessages();

      beforeEach(() => {
        act(() => {
          simulateMessageFromServer({
            type: 'fetchedDiffSummaries',
            summaries: {
              error: new Error('something failed'),
            },
          });
        });
      });

      it('shows error', () => {
        expect(screen.queryByText('something failed')).toBeInTheDocument();
        expect(screen.queryAllByTestId('github-error')).toHaveLength(2);
      });
    });

    it('can recover from errors', () => {
      const successful = {
        value: new Map<DiffId, DiffSummary>([
          [
            '10',
            {
              number: '10',
              state: PullRequestState.Open,
              title: 'PR A',
              type: 'github',
              url: 'https://github.com/myusername/testrepo/pull/10',
              anyUnresolvedComments: false,
              commentCount: 0,
            },
          ],
        ]),
      };

      const error = {error: new Error('something failed')};

      // start with success
      act(() => {
        simulateMessageFromServer({
          type: 'fetchedDiffSummaries',
          summaries: successful,
        });
      });
      expect(screen.queryByText('something failed')).not.toBeInTheDocument();
      expect(screen.queryAllByTestId('github-error')).toHaveLength(0);
      expect(
        within(within(screen.getByTestId('commit-a')).getByTestId('github-diff-info')).queryByText(
          '#10',
        ),
      ).toBeInTheDocument();

      // simulate error
      act(() => {
        simulateMessageFromServer({
          type: 'fetchedDiffSummaries',
          summaries: error,
        });
      });
      expect(screen.queryByText('something failed')).toBeInTheDocument();
      expect(screen.queryAllByTestId('github-error')).toHaveLength(2);

      // recover from error: get normal data again
      act(() => {
        simulateMessageFromServer({
          type: 'fetchedDiffSummaries',
          summaries: successful,
        });
      });
      expect(screen.queryByText('something failed')).not.toBeInTheDocument();
      expect(screen.queryAllByTestId('github-error')).toHaveLength(0);
      expect(
        within(within(screen.getByTestId('commit-a')).getByTestId('github-diff-info')).queryByText(
          '#10',
        ),
      ).toBeInTheDocument();
    });
  });
});
