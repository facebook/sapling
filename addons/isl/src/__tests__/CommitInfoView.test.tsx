/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hash} from '../types';

import App from '../App';
import * as commitMessageFields from '../CommitInfoView/CommitMessageFields';
import platform from '../platform';
import {CommitInfoTestUtils, ignoreRTL} from '../testQueries';
import {
  resetTestMessages,
  expectMessageSentToServer,
  simulateCommits,
  COMMIT,
  closeCommitInfoSidebar,
  simulateUncommittedChangedFiles,
  simulateMessageFromServer,
  openCommitInfoSidebar,
} from '../testUtils';
import {CommandRunner, succeedableRevset} from '../types';
import {fireEvent, render, screen, waitFor, within} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import {act} from 'react-dom/test-utils';

/* eslint-disable @typescript-eslint/no-non-null-assertion */

jest.mock('../MessageBus');

const {
  withinCommitInfo,
  clickAmendButton,
  clickCancel,
  clickCommitButton,
  clickCommitMode,
  clickToSelectCommit,
  getTitleEditor,
  getDescriptionEditor,
  clickToEditTitle,
  clickToEditDescription,
  expectIsEditingTitle,
  expectIsNOTEditingTitle,
  expectIsEditingDescription,
  expectIsNOTEditingDescription,
} = CommitInfoTestUtils;

describe('CommitInfoView', () => {
  beforeEach(() => {
    resetTestMessages();
    // Use OSS message fields for tests, even internally for consistency
    jest
      .spyOn(commitMessageFields, 'getDefaultCommitMessageSchema')
      .mockImplementation(() => commitMessageFields.OSSDefaultFieldSchema);
  });

  it('shows loading spinner on mount', () => {
    render(<App />);

    expect(screen.getByTestId('commit-info-view-loading')).toBeInTheDocument();
  });

  describe('after commits loaded', () => {
    beforeEach(() => {
      render(<App />);
      act(() => {
        openCommitInfoSidebar();
        expectMessageSentToServer({
          type: 'subscribe',
          kind: 'smartlogCommits',
          subscriptionID: expect.anything(),
        });
        simulateCommits({
          value: [
            COMMIT('1', 'some public base', '0', {phase: 'public'}),
            COMMIT('a', 'My Commit', '1'),
            COMMIT('b', 'Head Commit', 'a', {isHead: true}),
          ],
        });
      });
    });

    describe('drawer', () => {
      it('can close commit info sidebar by clicking label', () => {
        expect(screen.getByTestId('commit-info-view')).toBeInTheDocument();
        expect(screen.getByText('Commit Info')).toBeInTheDocument();
        act(() => {
          closeCommitInfoSidebar();
        });
        expect(screen.queryByTestId('commit-info-view')).not.toBeInTheDocument();
        expect(screen.getByText('Commit Info')).toBeInTheDocument();
      });
    });

    describe('commit selection', () => {
      it('shows head commit by default', () => {
        expect(withinCommitInfo().queryByText('Head Commit')).toBeInTheDocument();
      });

      it('can click to select commit', () => {
        clickToSelectCommit('a');

        // now commit info view shows selected commit
        expect(withinCommitInfo().queryByText('My Commit')).toBeInTheDocument();
        expect(withinCommitInfo().queryByText('Head Commit')).not.toBeInTheDocument();
      });

      it('cannot select public commits', () => {
        clickToSelectCommit('1');

        expect(withinCommitInfo().queryByText('some public base')).not.toBeInTheDocument();
        // stays on head commit
        expect(withinCommitInfo().queryByText('Head Commit')).toBeInTheDocument();
      });
    });

    describe('changed files', () => {
      beforeEach(() => {
        act(() => {
          simulateCommits({
            value: [
              COMMIT('1', 'some public base', '0', {phase: 'public'}),
              COMMIT('a', 'My Commit', '1', {filesSample: [{path: 'src/ca.js', status: 'M'}]}),
              COMMIT('b', 'Head Commit', 'a', {
                isHead: true,
                filesSample: [{path: 'src/cb.js', status: 'M'}],
                totalFileCount: 1,
              }),
            ],
          });
          simulateUncommittedChangedFiles({
            value: [
              {path: 'src/file1.js', status: 'M'},
              {path: 'src/file2.js', status: 'M'},
            ],
          });
        });
      });

      it('shows uncommitted changes for head commit', () => {
        expect(withinCommitInfo().queryByText(ignoreRTL('file1.js'))).toBeInTheDocument();
        expect(withinCommitInfo().queryByText(ignoreRTL('file2.js'))).toBeInTheDocument();
      });

      it('shows file actions on uncommitted changes in commit info view', () => {
        // (1) uncommitted changes in commit list, (2) uncommitted changes in commit info, (3) committed changes in commit info
        expect(withinCommitInfo().queryAllByTestId('file-actions')).toHaveLength(3);
      });

      it("doesn't show uncommitted changes on non-head commits ", () => {
        clickToSelectCommit('a');
        expect(withinCommitInfo().queryByText(ignoreRTL('file1.js'))).not.toBeInTheDocument();
        expect(withinCommitInfo().queryByText(ignoreRTL('file2.js'))).not.toBeInTheDocument();
      });

      it('shows files changed in the commit for head commit', () => {
        expect(withinCommitInfo().queryByText(ignoreRTL('ca.js'))).not.toBeInTheDocument();
        expect(withinCommitInfo().queryByText(ignoreRTL('cb.js'))).toBeInTheDocument();
      });

      it('shows files changed in the commit for non-head commit', () => {
        clickToSelectCommit('a');
        expect(withinCommitInfo().queryByText(ignoreRTL('ca.js'))).toBeInTheDocument();
        expect(withinCommitInfo().queryByText(ignoreRTL('cb.js'))).not.toBeInTheDocument();
      });

      it('enables amend button with uncommitted changes', () => {
        expect(withinCommitInfo().queryByText(ignoreRTL('file1.js'))).toBeInTheDocument();
        expect(withinCommitInfo().queryByText(ignoreRTL('file2.js'))).toBeInTheDocument();

        const amendButton: HTMLButtonElement | null = within(
          screen.getByTestId('commit-info-actions-bar'),
        ).queryByText('Amend');
        expect(amendButton).toBeInTheDocument();
        expect(amendButton?.disabled).not.toBe(true);
      });

      it('does not show banner if all files are shown', () => {
        expect(
          withinCommitInfo().queryByText(/Showing first .* files out of .* total/),
        ).not.toBeInTheDocument();
      });

      it('shows banner if not all files are shown', () => {
        act(() => {
          simulateCommits({
            value: [
              COMMIT('1', 'some public base', '0', {phase: 'public'}),
              COMMIT('a', 'Head Commit', '1', {
                isHead: true,
                filesSample: new Array(25)
                  .fill(null)
                  .map((_, i) => ({path: `src/file${i}.txt`, status: 'M'})),
                totalFileCount: 100,
              }),
            ],
          });
          simulateUncommittedChangedFiles({
            value: [],
          });
        });

        expect(withinCommitInfo().queryByText(ignoreRTL('file1.txt'))).toBeInTheDocument();
        expect(
          withinCommitInfo().queryByText('Showing first 25 files out of 100 total'),
        ).toBeInTheDocument();
      });

      it('runs amend with selected files', async () => {
        expect(withinCommitInfo().queryByText(ignoreRTL('file1.js'))).toBeInTheDocument();
        expect(withinCommitInfo().queryByText(ignoreRTL('file2.js'))).toBeInTheDocument();

        act(() => {
          const checkboxes = withinCommitInfo()
            .queryByTestId('changes-to-amend')!
            .querySelectorAll('input[type="checkbox"]');
          fireEvent.click(checkboxes[0]);
        });

        const amendButton = within(screen.getByTestId('commit-info-actions-bar')).queryByText(
          'Amend',
        );
        expect(amendButton).toBeInTheDocument();

        act(() => {
          fireEvent.click(amendButton!);
        });

        await waitFor(() =>
          expectMessageSentToServer({
            type: 'runOperation',
            operation: {
              args: [
                {type: 'config', key: 'amend.autorestack', value: 'always'},
                'amend',
                '--addremove',
                {type: 'repo-relative-file', path: 'src/file2.js'},
                '--message',
                expect.stringMatching(/^Head Commit/),
              ],
              id: expect.anything(),
              runner: CommandRunner.Sapling,
              trackEventName: 'AmendOperation',
            },
          }),
        );
      });

      it('disallows amending when all uncommitted changes deselected', () => {
        // click every checkbox in changes to amend
        act(() => {
          const checkboxes = withinCommitInfo()
            .queryByTestId('changes-to-amend')
            ?.querySelectorAll('input[type="checkbox"]');
          checkboxes?.forEach(checkbox => {
            fireEvent.click(checkbox);
          });
        });

        const amendButton: HTMLButtonElement | null = within(
          screen.getByTestId('commit-info-actions-bar'),
        ).queryByText('Amend');
        expect(amendButton).toBeInTheDocument();
        expect(amendButton?.disabled).toBe(true);
      });

      it('shows optimistic uncommitted changes', async () => {
        act(() => {
          simulateUncommittedChangedFiles({
            value: [],
          });
        });

        expect(screen.queryByText('Amend and Submit')).not.toBeInTheDocument();

        jest.spyOn(platform, 'confirm').mockImplementation(() => Promise.resolve(true));
        act(() => {
          fireEvent.click(screen.getByText('Uncommit'));
        });

        await waitFor(() => {
          expect(withinCommitInfo().queryByText(ignoreRTL('cb.js'))).toBeInTheDocument();
          expect(screen.queryByText('Amend and Submit')).toBeInTheDocument();
        });
      });
    });

    describe('editing fields', () => {
      beforeEach(() => {
        act(() => {
          simulateCommits({
            value: [
              COMMIT('1', 'some public base', '0', {phase: 'public'}),
              COMMIT('a', 'My Commit', '1', {description: 'Summary: First commit in the stack'}),
              COMMIT('b', 'Head Commit', 'a', {
                description: 'Summary: stacked commit',
                isHead: true,
              }),
            ],
          });
        });
      });

      it('starts editing title when clicked', () => {
        expectIsNOTEditingTitle();
        clickToEditTitle();
        expectIsEditingTitle();
      });

      it('starts editing description when clicked', () => {
        expectIsNOTEditingDescription();
        clickToEditDescription();
        expectIsEditingDescription();
      });

      it('cancel button stops editing', () => {
        clickToEditTitle();
        clickToEditDescription();
        expectIsEditingTitle();
        expectIsEditingDescription();

        const cancelButton: HTMLButtonElement | null = withinCommitInfo().queryByText('Cancel');
        expect(cancelButton).toBeInTheDocument();

        act(() => {
          fireEvent.click(cancelButton!);
        });

        expectIsNOTEditingTitle();
        expectIsNOTEditingDescription();
      });

      it('amend button stops editing', async () => {
        act(() =>
          simulateUncommittedChangedFiles({
            value: [{path: 'src/file1.js', status: 'M'}],
          }),
        );

        clickToEditTitle();
        clickToEditDescription();
        expectIsEditingTitle();
        expectIsEditingDescription();

        const amendButton: HTMLButtonElement | null = within(
          screen.getByTestId('commit-info-actions-bar'),
        ).queryByText('Amend');
        expect(amendButton).toBeInTheDocument();
        expect(amendButton?.disabled).not.toEqual(true);

        act(() => {
          fireEvent.click(amendButton!);
        });

        await waitFor(() =>
          expectMessageSentToServer({
            type: 'runOperation',
            operation: expect.objectContaining({
              args: expect.arrayContaining(['amend']),
            }),
          }),
        );

        expectIsNOTEditingTitle();
        expectIsNOTEditingDescription();
      });

      it('resets edited fields when changing selected commit', () => {
        clickToEditTitle();
        clickToEditDescription();
        expectIsEditingTitle();
        expectIsEditingDescription();

        clickToSelectCommit('a');

        expectIsNOTEditingTitle();
        expectIsNOTEditingDescription();
      });

      it('fields stay reset after switching commit, if there were no real changes made', () => {
        clickToEditTitle();
        clickToEditDescription();
        expectIsEditingTitle();
        expectIsEditingDescription();

        clickToSelectCommit('a');

        expectIsNOTEditingTitle();
        expectIsNOTEditingDescription();

        clickToSelectCommit('b');

        expectIsNOTEditingTitle();
        expectIsNOTEditingDescription();
      });

      it('edited fields go back into editing state when returning to selected commit', () => {
        clickToEditTitle();
        clickToEditDescription();
        expectIsEditingTitle();
        expectIsEditingDescription();

        {
          act(() => {
            userEvent.type(getTitleEditor(), ' hello new title');
            userEvent.type(getDescriptionEditor(), '\nhello new text');
          });
        }

        clickToSelectCommit('a');

        expectIsNOTEditingTitle();
        expectIsNOTEditingDescription();

        clickToSelectCommit('b');

        expectIsEditingTitle();
        expectIsEditingDescription();

        {
          expect(getTitleEditor().value).toBe('Head Commit hello new title');
          expect(getDescriptionEditor().value).toEqual(
            expect.stringContaining('stacked commit\nhello new text'),
          );
        }
      });

      it('cannot type newlines into title', () => {
        clickToEditTitle();
        expectIsEditingTitle();

        act(() => {
          userEvent.type(getTitleEditor(), ' hello\nsomething\r\nhi');
        });
        expect(getTitleEditor().value).toBe('Head Commit hellosomethinghi');
      });

      describe('focus', () => {
        it('focuses title when you start editing', async () => {
          clickToEditTitle();

          await waitFor(() => {
            expect(getTitleEditor()).toHaveFocus();
          });
        });

        it('focuses summary when you start editing', async () => {
          clickToEditDescription();

          await waitFor(() => {
            expect(getDescriptionEditor()).toHaveFocus();
          });
        });

        it('focuses topmost field (title) when both title and description start being edited simultaneously', async () => {
          // edit fields, then switch selected commit and switch back to edit both fields together
          clickToEditTitle();
          clickToEditDescription();
          {
            act(() => {
              userEvent.type(getTitleEditor(), ' hello new title');
              userEvent.type(getDescriptionEditor(), '\nhello new text');
            });
          }

          clickToSelectCommit('a');
          clickToSelectCommit('b');

          expectIsEditingTitle();
          expectIsEditingDescription();

          await waitFor(() => {
            expect(getTitleEditor()).toHaveFocus();
            expect(getDescriptionEditor()).not.toHaveFocus();
          });
        });
      });

      describe('head commit', () => {
        it('only has metaedit button on non-head commits', () => {
          {
            const preChangeAmendMessageButton = within(
              screen.getByTestId('commit-info-actions-bar'),
            ).queryByText('Amend Message');
            expect(preChangeAmendMessageButton).not.toBeInTheDocument();
          }

          clickToSelectCommit('a');

          const amendMessageButton = within(
            screen.getByTestId('commit-info-actions-bar'),
          ).queryByText('Amend Message');
          expect(amendMessageButton).toBeInTheDocument();
        });

        it('has "You are here" on head commit', () => {
          expect(withinCommitInfo().queryByText('You are here')).toBeInTheDocument();
        });

        it('does not have "You are here" on non-head commit', () => {
          clickToSelectCommit('a');
          expect(withinCommitInfo().queryByText('You are here')).not.toBeInTheDocument();
        });

        it('does not have "You are here" in commit mode', () => {
          clickCommitMode();
          expect(withinCommitInfo().queryByText('You are here')).not.toBeInTheDocument();
        });
      });

      describe('running commands', () => {
        describe('metaedit', () => {
          it('disables metaedit button if no fields edited', () => {
            clickToSelectCommit('a');

            const amendMessageButton = within(
              screen.getByTestId('commit-info-actions-bar'),
            ).queryByText('Amend Message') as HTMLButtonElement;
            expect(amendMessageButton).toBeInTheDocument();
            expect(amendMessageButton!.disabled).toBe(true);
          });

          it('enables metaedit button if fields are edited', () => {
            clickToSelectCommit('a');

            clickToEditTitle();
            clickToEditDescription();
          });

          it('runs metaedit', async () => {
            clickToSelectCommit('a');

            clickToEditTitle();
            clickToEditDescription();

            {
              act(() => {
                userEvent.type(getTitleEditor(), ' hello new title');
                userEvent.type(getDescriptionEditor(), '\nhello new text');
              });
            }

            const amendMessageButton = within(
              screen.getByTestId('commit-info-actions-bar'),
            ).queryByText('Amend Message');
            act(() => {
              fireEvent.click(amendMessageButton!);
            });
            await waitFor(() =>
              expectMessageSentToServer({
                type: 'runOperation',
                operation: {
                  args: [
                    'metaedit',
                    '--rev',
                    succeedableRevset('a'),
                    '--message',
                    expect.stringMatching(
                      /^My Commit hello new title\n+Summary: First commit in the stack\nhello new text/,
                    ),
                  ],
                  id: expect.anything(),
                  runner: CommandRunner.Sapling,
                  trackEventName: 'AmendMessageOperation',
                },
              }),
            );
          });

          it('disables metaedit button with spinner while running', async () => {
            clickToSelectCommit('a');

            clickToEditTitle();
            clickToEditDescription();
            {
              act(() => {
                userEvent.type(getTitleEditor(), ' hello new title');
                userEvent.type(getDescriptionEditor(), '\nhello new text');
              });
            }

            const amendMessageButton = within(
              screen.getByTestId('commit-info-actions-bar'),
            ).queryByText('Amend Message');
            act(() => {
              fireEvent.click(amendMessageButton!);
            });

            await waitFor(() => expect(amendMessageButton).toBeDisabled());
          });
        });

        describe('amend', () => {
          it('runs amend with changed message', async () => {
            act(() =>
              simulateUncommittedChangedFiles({
                value: [{path: 'src/file1.js', status: 'M'}],
              }),
            );

            clickToEditTitle();
            clickToEditDescription();

            {
              act(() => {
                userEvent.type(getTitleEditor(), ' hello new title');
                userEvent.type(getDescriptionEditor(), '\nhello new text');
              });
            }

            clickAmendButton();

            await waitFor(() =>
              expectMessageSentToServer({
                type: 'runOperation',
                operation: {
                  args: [
                    {type: 'config', key: 'amend.autorestack', value: 'always'},
                    'amend',
                    '--addremove',
                    '--message',
                    expect.stringMatching(
                      /^Head Commit hello new title\n+Summary: stacked commit\nhello new text/,
                    ),
                  ],
                  id: expect.anything(),
                  runner: CommandRunner.Sapling,
                  trackEventName: 'AmendOperation',
                },
              }),
            );
          });

          it('deselects head when running amend', async () => {
            act(() =>
              simulateUncommittedChangedFiles({
                value: [{path: 'src/file1.js', status: 'M'}],
              }),
            );

            // even though 'b' is already shown when nothing is selected,
            // we want to use auto-selection after amending even if you previously selected something
            clickToSelectCommit('b');

            clickToEditTitle();

            {
              act(() => {
                userEvent.type(getTitleEditor(), ' hello');
              });
            }

            clickAmendButton();

            // no commit is selected anymore
            await waitFor(() =>
              expect(screen.queryByTestId('selected-commit')).not.toBeInTheDocument(),
            );
          });

          it('does not deselect non-head commits after running amend', () => {
            act(() => {
              simulateUncommittedChangedFiles({
                value: [{path: 'src/file1.js', status: 'M'}],
              });
            });

            clickToSelectCommit('a');

            // we can't use amend button in the commit info because we're on some other commit, so use quick amend
            const quickAmendButton = screen.getByTestId('uncommitted-changes-quick-amend-button');
            act(() => {
              fireEvent.click(quickAmendButton);
            });

            // commit remains selected
            expect(
              within(screen.getByTestId('commit-a')).queryByTestId('selected-commit'),
            ).toBeInTheDocument();
          });

          it('disables amend button with spinner while running', async () => {
            act(() => {
              simulateUncommittedChangedFiles({
                value: [{path: 'src/file1.js', status: 'M'}],
              });
            });

            clickAmendButton();
            await waitFor(() =>
              expectMessageSentToServer({
                type: 'runOperation',
                operation: expect.objectContaining({
                  args: expect.arrayContaining(['amend']),
                }),
              }),
            );

            act(() => {
              simulateUncommittedChangedFiles({
                value: [{path: 'src/file2.js', status: 'M'}],
              });
            });

            const amendMessageButton = within(
              screen.getByTestId('commit-info-actions-bar'),
            ).queryByText('Amend');
            act(() => {
              fireEvent.click(amendMessageButton!);
            });

            expect(amendMessageButton).toBeDisabled();
          });

          it('shows amend message instead of amend when there are only message changes', () => {
            act(() => {
              simulateUncommittedChangedFiles({
                value: [{path: 'src/file1.js', status: 'M'}],
              });
            });

            expect(
              within(screen.getByTestId('commit-info-actions-bar')).queryByText('Amend'),
            ).toBeInTheDocument();

            act(() => {
              simulateUncommittedChangedFiles({
                value: [],
              });
            });

            expect(
              within(screen.getByTestId('commit-info-actions-bar')).queryByText('Amend'),
            ).toBeInTheDocument();

            clickToEditTitle();
            clickToEditDescription();

            // no uncommitted changes, and message is being changed
            expect(
              within(screen.getByTestId('commit-info-actions-bar')).queryByText('Amend Message'),
            ).toBeInTheDocument();
          });
        });
      });

      describe('commit mode', () => {
        it('has commit mode selector on head commit', () => {
          expect(
            within(screen.getByTestId('commit-info-toolbar-top')).getByText('Amend'),
          ).toBeInTheDocument();
          expect(
            within(screen.getByTestId('commit-info-toolbar-top')).getByText('Commit'),
          ).toBeInTheDocument();
        });

        it('does not have commit mode selector on non-head commits', () => {
          clickToSelectCommit('a');
          // toolbar won't appear at all on non-head commits right now
          expect(screen.queryByTestId('commit-info-toolbar-top')).not.toBeInTheDocument();
        });

        it('clicking commit mode starts editing both fields', () => {
          clickCommitMode();

          expectIsEditingTitle();
          expectIsEditingDescription();
        });

        it('commit mode message is saved separately', () => {
          clickCommitMode();

          expectIsEditingTitle();
          expectIsEditingDescription();

          act(() => {
            userEvent.type(getTitleEditor(), 'new commit title');
            userEvent.type(getDescriptionEditor(), 'my description');
          });

          clickToSelectCommit('a');
          clickToSelectCommit('b');

          expectIsEditingTitle();
          expectIsEditingDescription();

          expect(getTitleEditor().value).toBe('new commit title');
          expect(getDescriptionEditor().value).toBe('my description');
        });

        it('focuses title when opening commit mode', async () => {
          clickCommitMode();

          await waitFor(() => {
            expect(getTitleEditor()).toHaveFocus();
            expect(getDescriptionEditor()).not.toHaveFocus();
          });
        });

        it('disables commit button if theres no changed files', () => {
          clickCommitMode();

          const commitButton = within(screen.getByTestId('commit-info-actions-bar')).queryByText(
            'Commit',
          ) as HTMLButtonElement;
          expect(commitButton).toBeInTheDocument();
          expect(commitButton!.disabled).toBe(true);
        });

        it('does not disable commit button if there are changed files', () => {
          act(() => {
            simulateUncommittedChangedFiles({
              value: [
                {path: 'src/file1.js', status: 'M'},
                {path: 'src/file2.js', status: 'M'},
              ],
            });
          });

          clickCommitMode();

          const commitButton = within(screen.getByTestId('commit-info-actions-bar')).queryByText(
            'Commit',
          ) as HTMLButtonElement;
          expect(commitButton).toBeInTheDocument();
          expect(commitButton!.disabled).not.toBe(true);
        });

        it('runs commit with message', async () => {
          act(() => {
            simulateUncommittedChangedFiles({
              value: [
                {path: 'src/file1.js', status: 'M'},
                {path: 'src/file2.js', status: 'M'},
              ],
            });
          });

          clickCommitMode();

          act(() => {
            userEvent.type(getTitleEditor(), 'new commit title');
            userEvent.type(getDescriptionEditor(), 'my description');
          });

          clickCommitButton();

          await waitFor(() =>
            expectMessageSentToServer({
              type: 'runOperation',
              operation: {
                args: [
                  'commit',
                  '--addremove',
                  '--message',
                  expect.stringMatching(/^new commit title\n+(Summary: )?my description/),
                ],
                id: expect.anything(),
                runner: CommandRunner.Sapling,
                trackEventName: 'CommitOperation',
              },
            }),
          );
        });

        it('resets to amend mode after committing', async () => {
          act(() => {
            simulateUncommittedChangedFiles({
              value: [
                {path: 'src/file1.js', status: 'M'},
                {path: 'src/file2.js', status: 'M'},
              ],
            });
          });

          clickCommitMode();

          act(() => {
            userEvent.type(getTitleEditor(), 'new commit title');
            userEvent.type(getDescriptionEditor(), 'my description');
          });

          clickCommitButton();

          await waitFor(() =>
            expectMessageSentToServer({
              type: 'runOperation',
              operation: expect.objectContaining({
                args: expect.arrayContaining(['commit']),
              }),
            }),
          );

          const commitButtonAfter = within(
            screen.getByTestId('commit-info-actions-bar'),
          ).queryByText('Commit');
          expect(commitButtonAfter).not.toBeInTheDocument();
        });

        it('does not have cancel button', () => {
          clickCommitMode();

          const cancelButton: HTMLButtonElement | null = withinCommitInfo().queryByText('Cancel');
          expect(cancelButton).not.toBeInTheDocument();
        });
      });

      describe('edited messages indicator', () => {
        it('does not show edited message indicator when fields are not actually changed', () => {
          clickToEditTitle();
          clickToEditDescription();
          expectIsEditingTitle();
          expectIsEditingDescription();

          expect(screen.queryByTestId('unsaved-message-indicator')).not.toBeInTheDocument();

          {
            act(() => {
              // type something and delete it
              userEvent.type(getTitleEditor(), 'Q{Backspace}');
              userEvent.type(getDescriptionEditor(), 'Q{Backspace}');
            });
          }

          expect(screen.queryByTestId('unsaved-message-indicator')).not.toBeInTheDocument();
        });

        it('shows edited message indicator when title changed', () => {
          clickToEditTitle();
          clickToEditDescription();

          expect(screen.queryByTestId('unsaved-message-indicator')).not.toBeInTheDocument();

          {
            act(() => {
              userEvent.type(getTitleEditor(), 'Q');
            });
          }

          expect(screen.queryByTestId('unsaved-message-indicator')).toBeInTheDocument();
          expect(
            within(screen.queryByTestId('commit-b')!).queryByTestId('unsaved-message-indicator'),
          ).toBeInTheDocument();
        });

        it('shows edited message indicator when description changed', () => {
          clickToEditTitle();
          clickToEditDescription();

          expect(screen.queryByTestId('unsaved-message-indicator')).not.toBeInTheDocument();

          {
            act(() => {
              userEvent.type(getDescriptionEditor(), 'Q');
            });
          }

          expect(screen.queryByTestId('unsaved-message-indicator')).toBeInTheDocument();
          expect(
            within(screen.queryByTestId('commit-b')!).queryByTestId('unsaved-message-indicator'),
          ).toBeInTheDocument();
        });

        it('appears for other commits', () => {
          clickToSelectCommit('a');

          clickToEditTitle();
          clickToEditDescription();

          expect(screen.queryByTestId('unsaved-message-indicator')).not.toBeInTheDocument();

          {
            act(() => {
              userEvent.type(getTitleEditor(), 'Q');
              userEvent.type(getDescriptionEditor(), 'Q');
            });
          }

          expect(screen.queryByTestId('unsaved-message-indicator')).toBeInTheDocument();
          expect(
            within(screen.queryByTestId('commit-a')!).queryByTestId('unsaved-message-indicator'),
          ).toBeInTheDocument();
        });

        it('commit mode does not cause indicator', () => {
          clickCommitMode();

          {
            act(() => {
              userEvent.type(getTitleEditor(), 'Q');
              userEvent.type(getDescriptionEditor(), 'Q');
            });
          }
          expect(screen.queryByTestId('unsaved-message-indicator')).not.toBeInTheDocument();
        });
      });

      describe('commit message template', () => {
        it('requests commit template when opening commit form', () => {
          clickCommitMode();
          expectMessageSentToServer({type: 'fetchCommitMessageTemplate'});
        });

        it('loads template sent by server', () => {
          clickCommitMode();
          act(() => {
            simulateMessageFromServer({
              type: 'fetchedCommitMessageTemplate',
              template: '[isl]\nSummary: Hello\nTest Plan:\n',
            });
          });

          expect(getTitleEditor().value).toBe('[isl]');
          expect(getDescriptionEditor().value).toEqual(expect.stringMatching(/(Summary: )?Hello/));
        });

        it('only asynchronously overwrites default commit fields', () => {
          clickCommitMode();

          // type something in fields...
          {
            act(() => {
              userEvent.type(getTitleEditor(), 'Q');
              userEvent.type(getDescriptionEditor(), 'W');
            });
          }

          // template arrives from server later
          act(() => {
            simulateMessageFromServer({
              type: 'fetchedCommitMessageTemplate',
              template: '[isl]\nSummary:\nTest Plan:\n',
            });
          });

          // template shouldn't have overwritten fields since they're non-default now
          expect(getTitleEditor().value).toBe('Q');
          expect(getDescriptionEditor().value).toBe('W');
        });
      });

      describe('discarding message', () => {
        it('confirms cancel button if you have made changes to the title', async () => {
          clickToEditTitle();
          const confirmSpy = jest
            .spyOn(platform, 'confirm')
            .mockImplementation(() => Promise.resolve(true));

          act(() => {
            userEvent.type(getTitleEditor(), 'Q');
          });

          clickCancel();

          await waitFor(() => {
            expectIsNOTEditingTitle();
            expectIsNOTEditingDescription();
          });
          expect(confirmSpy).toHaveBeenCalled();
        });

        it('confirms cancel button if you have made changes to the description', async () => {
          clickToEditDescription();
          const confirmSpy = jest
            .spyOn(platform, 'confirm')
            .mockImplementation(() => Promise.resolve(true));

          act(() => {
            userEvent.type(getDescriptionEditor(), 'W');
          });

          clickCancel();

          await waitFor(() => {
            expectIsNOTEditingTitle();
            expectIsNOTEditingDescription();
          });
          expect(confirmSpy).toHaveBeenCalled();
        });

        it('does not cancel if you do not confirm', async () => {
          clickToEditTitle();
          clickToEditDescription();
          const confirmSpy = jest
            .spyOn(platform, 'confirm')
            .mockImplementation(() => Promise.resolve(false));

          act(() => {
            userEvent.type(getTitleEditor(), 'Q');
            userEvent.type(getDescriptionEditor(), 'W');
          });

          clickCancel();

          await waitFor(() => {
            expectIsEditingTitle();
            expectIsEditingDescription();

            expect(getTitleEditor().value).toBe('Head CommitQ');
            expect(getDescriptionEditor().value).toEqual(
              expect.stringContaining('stacked commitW'),
            );
          });
          expect(confirmSpy).toHaveBeenCalled();
        });

        it('does not confirm when clearing for amend', async () => {
          act(() =>
            simulateUncommittedChangedFiles({
              value: [{path: 'src/file1.js', status: 'M'}],
            }),
          );

          clickToEditDescription();
          const confirmSpy = jest.spyOn(platform, 'confirm');

          act(() => {
            userEvent.type(getDescriptionEditor(), 'W');
          });

          clickAmendButton();

          await waitFor(() => {
            expectIsNOTEditingTitle();
            expectIsNOTEditingDescription();
            expect(confirmSpy).not.toHaveBeenCalled();
          });
        });
      });

      describe('optimistic state', () => {
        const clickGotoCommit = (hash: Hash) => {
          const gotoButton = within(screen.getByTestId(`commit-${hash}`)).getByText('Goto');
          fireEvent.click(gotoButton);
        };

        it('takes previews into account when rendering head', () => {
          clickGotoCommit('a');
          // while optimistic state happening...
          // show new commit in commit info without clicking it (because head is auto-selected)
          expect(withinCommitInfo().queryByText('My Commit')).toBeInTheDocument();
          expect(withinCommitInfo().queryByText('You are here')).toBeInTheDocument();
        });

        it('shows new head when running goto', () => {
          clickToSelectCommit('b'); // explicitly select
          clickGotoCommit('a');

          expect(withinCommitInfo().queryByText('My Commit')).toBeInTheDocument();
          expect(withinCommitInfo().queryByText('You are here')).toBeInTheDocument();
        });

        it('renders metaedit operation smoothly', async () => {
          clickToSelectCommit('a');

          clickToEditTitle();
          clickToEditDescription();
          act(() => {
            userEvent.type(getTitleEditor(), ' with change!');
            userEvent.type(getDescriptionEditor(), '\nmore stuff!');
          });

          const amendMessageButton = within(
            screen.getByTestId('commit-info-actions-bar'),
          ).queryByText('Amend Message');
          act(() => {
            fireEvent.click(amendMessageButton!);
          });

          await waitFor(() => {
            expectIsNOTEditingTitle();
            expectIsNOTEditingDescription();

            expect(withinCommitInfo().getByText('My Commit with change!')).toBeInTheDocument();
            expect(
              withinCommitInfo().getByText(/First commit in the stack\nmore stuff!/, {
                collapseWhitespace: false,
              }),
            ).toBeInTheDocument();
          });
        });

        it('renders commit operation smoothly', async () => {
          act(() => {
            simulateUncommittedChangedFiles({
              value: [{path: 'src/file1.js', status: 'M'}],
            });
          });

          clickCommitMode();
          act(() => {
            userEvent.type(getTitleEditor(), 'New Commit');
            userEvent.type(getDescriptionEditor(), 'Message!');
          });

          clickCommitButton();

          // optimistic state should now be rendered, so we show a fake commit with the new title,
          // but not in editing mode anymore

          await waitFor(() => {
            expectIsNOTEditingTitle();
            expectIsNOTEditingDescription();

            expect(withinCommitInfo().queryByText('New Commit')).toBeInTheDocument();
            expect(withinCommitInfo().queryByText(/Message!/)).toBeInTheDocument();
            expect(withinCommitInfo().queryByText('You are here')).toBeInTheDocument();
          });

          // finish commit operation with hg log
          act(() => {
            simulateCommits({
              value: [
                COMMIT('1', 'some public base', '0', {phase: 'public'}),
                COMMIT('a', 'My Commit', '1'),
                COMMIT('b', 'Head Commit', 'a'),
                COMMIT('c', 'New Commit', 'b', {
                  isHead: true,
                  description: 'Summary: Message!',
                }),
              ],
            });
          });
          expect(withinCommitInfo().queryByText('New Commit')).toBeInTheDocument();
          expect(withinCommitInfo().getByText(/Message!/)).toBeInTheDocument();
          expect(withinCommitInfo().queryByText('You are here')).toBeInTheDocument();
        });

        it('doesnt let you edit on optimistic commit', async () => {
          act(() => {
            simulateUncommittedChangedFiles({
              value: [{path: 'src/file1.js', status: 'M'}],
            });
          });

          clickCommitMode();
          act(() => {
            userEvent.type(getTitleEditor(), 'New Commit');
            userEvent.type(getDescriptionEditor(), 'Message!');
          });
          clickCommitButton();

          await waitFor(() =>
            expectMessageSentToServer({
              type: 'runOperation',
              operation: expect.objectContaining({
                args: expect.arrayContaining(['commit']),
              }),
            }),
          );

          clickToEditTitle();
          clickToEditDescription();
          // cannot click to edit optimistic commit
          expectIsNOTEditingTitle();
          expectIsNOTEditingDescription();

          // finish commit operation with hg log
          act(() => {
            simulateCommits({
              value: [
                COMMIT('1', 'some public base', '0', {phase: 'public'}),
                COMMIT('a', 'My Commit', '1'),
                COMMIT('b', 'Head Commit', 'a'),
                COMMIT('c', 'New Commit', 'b', {isHead: true, description: 'Summary: Message!'}),
              ],
            });
          });

          clickToEditTitle();
          clickToEditDescription();
          // now you can edit just fine
          expectIsEditingTitle();
          expectIsEditingDescription();
        });

        it('renders amend operation smoothly', async () => {
          act(() =>
            simulateUncommittedChangedFiles({
              value: [{path: 'src/file1.js', status: 'M'}],
            }),
          );

          clickToEditTitle();
          clickToEditDescription();
          act(() => {
            userEvent.type(getTitleEditor(), ' Hey');
            userEvent.type(getDescriptionEditor(), '\nHello');
          });

          clickAmendButton();

          // optimistic state should now be rendered, so we update the head commit
          // but not in editing mode anymore

          await waitFor(() => {
            expectIsNOTEditingTitle();
            expectIsNOTEditingDescription();

            expect(withinCommitInfo().getByText('Head Commit Hey')).toBeInTheDocument();
            expect(
              withinCommitInfo().getByText(/stacked commit\nHello/, {
                collapseWhitespace: false,
              }),
            ).toBeInTheDocument();
            expect(withinCommitInfo().getByText('You are here')).toBeInTheDocument();
          });

          // finish amend operation with hg log
          act(() => {
            simulateCommits({
              value: [
                COMMIT('1', 'some public base', '0', {phase: 'public'}),
                COMMIT('a', 'My Commit', '1'),
                COMMIT('b2', 'Head Commit Hey', 'a', {
                  isHead: true,
                  description: 'Summary: stacked commit\nHello',
                }),
              ],
            });
          });
          expect(withinCommitInfo().getByText('Head Commit Hey')).toBeInTheDocument();
          expect(
            withinCommitInfo().getByText(/stacked commit\nHello/, {
              collapseWhitespace: false,
            }),
          ).toBeInTheDocument();
          expect(withinCommitInfo().getByText('You are here')).toBeInTheDocument();
        });
      });

      describe('Opening form in edit mode from uncommitted changes', () => {
        beforeEach(() => {
          act(() => {
            simulateUncommittedChangedFiles({
              value: [{path: 'src/file1.js', status: 'M'}],
            });
          });
        });

        const clickAmendAs = async () => {
          const amendAsButton = screen.getByText('Amend as...');
          act(() => {
            fireEvent.click(amendAsButton!);
          });

          await waitFor(() => {
            expectIsEditingTitle();
            expectIsEditingDescription();
          });
        };
        const clickCommitAs = () => {
          const commitAsButton = screen.getByText('Commit as...');
          act(() => {
            fireEvent.click(commitAsButton!);
          });
        };

        it('Opens form if closed', async () => {
          act(() => {
            closeCommitInfoSidebar();
          });

          await clickAmendAs();

          expect(screen.getByTestId('commit-info-view')).toBeInTheDocument();
        });

        it('Deselects so head commit is shown', async () => {
          clickToSelectCommit('a');
          await clickAmendAs();

          await waitFor(() => {
            // no commit is selected anymore
            expect(screen.queryByTestId('selected-commit')).not.toBeInTheDocument();
            expect(withinCommitInfo().queryByText('Head Commit')).toBeInTheDocument();
          });
        });

        describe('Amend as...', () => {
          it('Opens form in amend mode', async () => {
            clickCommitMode();
            await clickAmendAs();

            const amendButton: HTMLButtonElement | null = within(
              screen.getByTestId('commit-info-actions-bar'),
            ).queryByText('Amend');
            expect(amendButton).toBeInTheDocument();
          });

          it('starts editing fields', async () => {
            clickCommitMode();
            await clickAmendAs();

            await waitFor(() => {
              expectIsEditingTitle();
              expectIsEditingDescription();
            });
          });

          it('focuses fields', async () => {
            await clickAmendAs();

            await waitFor(() => {
              expect(getTitleEditor()).toHaveFocus();
            });
          });
        });

        describe('Commit as...', () => {
          it('Opens form in commit mode', () => {
            clickCommitAs();

            const commitButton: HTMLButtonElement | null = within(
              screen.getByTestId('commit-info-actions-bar'),
            ).queryByText('Commit');
            expect(commitButton).toBeInTheDocument();
          });

          it('focuses fields', async () => {
            clickCommitAs();

            await waitFor(() => {
              expect(getTitleEditor()).toHaveFocus();
            });
          });

          it('focuses fields even if amend fields already being edited', async () => {
            await clickAmendAs();

            await waitFor(() => {
              expect(getTitleEditor()).toHaveFocus();
            });
            expect(getTitleEditor().value).toEqual('Head Commit');

            act(() => {
              // explicitly blur title so "commit as" really has to focus it again
              getTitleEditor().blur();
            });

            clickCommitAs();

            await waitFor(() => {
              expect(getTitleEditor().value).toEqual('');
            });
            expect(getTitleEditor()).toHaveFocus();
          });

          it('copies commit title from quick commit form', () => {
            const title = screen.getByTestId('quick-commit-title');
            act(() => {
              userEvent.type(title, 'Hello, world!');
            });
            clickCommitAs();

            expect((screen.getByTestId('quick-commit-title') as HTMLInputElement).value).toEqual(
              '',
            );
            expect(getTitleEditor().value).toBe('Hello, world!');
          });
        });
      });
    });

    describe('Public commits in amend mode', () => {
      beforeEach(() => {
        act(() => {
          simulateCommits({
            value: [
              COMMIT('1', 'some public base', '0', {phase: 'public', isHead: true}),
              COMMIT('a', 'My Commit', '1'),
              COMMIT('b', 'Head Commit', 'a'),
            ],
          });
        });
      });

      it('shows public label', () => {
        expect(withinCommitInfo().getByText('Public')).toBeInTheDocument();
      });

      it('does not allow submitting', () => {
        expect(withinCommitInfo().queryByText('Submit')).not.toBeInTheDocument();
      });

      it('does not show changes to amend', () => {
        expect(withinCommitInfo().queryByText('Changes to Amend')).not.toBeInTheDocument();
      });

      it('does not allow clicking to edit fields', () => {
        expectIsNOTEditingTitle();
        expectIsNOTEditingDescription();

        clickToEditTitle();
        clickToEditDescription();

        expectIsNOTEditingTitle();
        expectIsNOTEditingDescription();
      });
    });
  });
});
