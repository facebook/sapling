/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hash} from '../types';

import App from '../App';
import platform from '../platform';
import {
  resetTestMessages,
  expectMessageSentToServer,
  simulateCommits,
  COMMIT,
  closeCommitInfoSidebar,
  simulateUncommittedChangedFiles,
  simulateMessageFromServer,
} from '../testUtils';
import {CommandRunner, SucceedableRevset} from '../types';
import {fireEvent, render, screen, waitFor, within} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import {act} from 'react-dom/test-utils';

/* eslint-disable @typescript-eslint/no-non-null-assertion */

jest.mock('../MessageBus');

describe('CommitInfoView', () => {
  beforeEach(() => {
    resetTestMessages();
  });

  it('shows loading spinner on mount', () => {
    render(<App />);

    expect(screen.getByTestId('commit-info-view-loading')).toBeInTheDocument();
  });

  function clickToSelectCommit(hash: string) {
    const commit = within(screen.getByTestId(`commit-${hash}`)).queryByTestId('draggable-commit');
    expect(commit).toBeInTheDocument();
    act(() => {
      fireEvent.click(commit!);
    });
  }

  const clickCommitMode = () => {
    const commitRadioChoice = within(screen.getByTestId('commit-info-toolbar-top')).getByText(
      'Commit',
    );
    act(() => {
      fireEvent.click(commitRadioChoice);
    });
  };

  const clickAmendButton = () => {
    const amendButton: HTMLButtonElement | null = within(
      screen.getByTestId('commit-info-actions-bar'),
    ).queryByText('Amend');
    expect(amendButton).toBeInTheDocument();
    act(() => {
      fireEvent.click(amendButton!);
    });
  };

  const clickCommitButton = () => {
    const commitButton: HTMLButtonElement | null = within(
      screen.getByTestId('commit-info-actions-bar'),
    ).queryByText('Commit');
    expect(commitButton).toBeInTheDocument();
    act(() => {
      fireEvent.click(commitButton!);
    });
  };

  const clickCancel = () => {
    const cancelButton: HTMLButtonElement | null = within(
      screen.getByTestId('commit-info-view'),
    ).queryByText('Cancel');
    expect(cancelButton).toBeInTheDocument();

    act(() => {
      fireEvent.click(cancelButton!);
    });
  };

  describe('after commits loaded', () => {
    beforeEach(() => {
      render(<App />);
      act(() => {
        expectMessageSentToServer({
          type: 'subscribeSmartlogCommits',
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
      it('starts with commit info open', () => {
        expect(screen.getByTestId('commit-info-view')).toBeInTheDocument();
        expect(screen.getByText('Commit Info')).toBeInTheDocument();
      });

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
        const commitInfoView = screen.getByTestId('commit-info-view');
        expect(within(commitInfoView).queryByText('Head Commit')).toBeInTheDocument();
      });

      it('can click to select commit', () => {
        clickToSelectCommit('a');

        // now commit info view shows selected commit
        const commitInfoView = screen.getByTestId('commit-info-view');
        expect(within(commitInfoView).queryByText('My Commit')).toBeInTheDocument();
        expect(within(commitInfoView).queryByText('Head Commit')).not.toBeInTheDocument();
      });

      it('cannot select public commits', () => {
        clickToSelectCommit('1');

        const commitInfoView = screen.getByTestId('commit-info-view');
        expect(within(commitInfoView).queryByText('some public base')).not.toBeInTheDocument();
        // stays on head commit
        expect(within(commitInfoView).queryByText('Head Commit')).toBeInTheDocument();
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
        const commitInfoView = screen.getByTestId('commit-info-view');
        expect(within(commitInfoView).queryByText('src/file1.js')).toBeInTheDocument();
        expect(within(commitInfoView).queryByText('src/file2.js')).toBeInTheDocument();
      });

      it('shows file actions on uncommitted changes in commit info view', () => {
        const commitInfoView = screen.getByTestId('commit-info-view');
        expect(within(commitInfoView).queryAllByTestId('file-actions')).toHaveLength(2);
      });

      it('does not show file actions on committed changes in commit info view', () => {
        clickToSelectCommit('a'); // non-head commit doesn't have uncommitted changes

        // now commit info view shows selected commit
        const commitInfoView = screen.getByTestId('commit-info-view');
        expect(within(commitInfoView).queryByTestId('file-actions')).not.toBeInTheDocument();
      });

      it("doesn't show uncommitted changes on non-head commits ", () => {
        clickToSelectCommit('a');
        const commitInfoView = screen.getByTestId('commit-info-view');
        expect(within(commitInfoView).queryByText('src/file1.js')).not.toBeInTheDocument();
        expect(within(commitInfoView).queryByText('src/file2.js')).not.toBeInTheDocument();
      });

      it('shows files changed in the commit for head commit', () => {
        const commitInfoView = screen.getByTestId('commit-info-view');
        expect(within(commitInfoView).queryByText('src/ca.js')).not.toBeInTheDocument();
        expect(within(commitInfoView).queryByText('src/cb.js')).toBeInTheDocument();
      });

      it('shows files changed in the commit for non-head commit', () => {
        clickToSelectCommit('a');
        const commitInfoView = screen.getByTestId('commit-info-view');
        expect(within(commitInfoView).queryByText('src/ca.js')).toBeInTheDocument();
        expect(within(commitInfoView).queryByText('src/cb.js')).not.toBeInTheDocument();
      });

      it('enables amend button with uncommitted changes', () => {
        const commitInfoView = screen.getByTestId('commit-info-view');
        expect(within(commitInfoView).queryByText('src/file1.js')).toBeInTheDocument();
        expect(within(commitInfoView).queryByText('src/file2.js')).toBeInTheDocument();

        const amendButton: HTMLButtonElement | null = within(
          screen.getByTestId('commit-info-actions-bar'),
        ).queryByText('Amend');
        expect(amendButton).toBeInTheDocument();
        expect(amendButton?.disabled).not.toBe(true);
      });

      it('runs amend with selected files', () => {
        const commitInfoView = screen.getByTestId('commit-info-view');
        expect(within(commitInfoView).queryByText('src/file1.js')).toBeInTheDocument();
        expect(within(commitInfoView).queryByText('src/file2.js')).toBeInTheDocument();

        act(() => {
          const checkboxes = within(commitInfoView)
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

        expectMessageSentToServer({
          type: 'runOperation',
          operation: {
            args: [
              'amend',
              {type: 'repo-relative-file', path: 'src/file2.js'},
              '--message',
              'Head Commit\n',
            ],
            id: expect.anything(),
            runner: CommandRunner.Sapling,
          },
        });
      });

      it('disallows amending when all uncommitted changes deselected', () => {
        const commitInfoView = screen.getByTestId('commit-info-view');

        // click every checkbox in changes to amend
        act(() => {
          const checkboxes = within(commitInfoView)
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

      it('shows optimistic uncommitted changes', () => {
        act(() => {
          simulateUncommittedChangedFiles({
            value: [],
          });
        });

        expect(screen.queryByText('Amend and Submit')).not.toBeInTheDocument();

        act(() => {
          fireEvent.click(screen.getByText('Uncommit'));
        });

        const commitInfoView = screen.getByTestId('commit-info-view');
        expect(within(commitInfoView).queryByText('src/cb.js')).toBeInTheDocument();
        expect(screen.queryByText('Amend and Submit')).toBeInTheDocument();
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

      const getTitleEditor = (): HTMLInputElement => {
        const title = screen.getByTestId('commit-info-title-field') as HTMLInputElement;
        expect(title).toBeInTheDocument();
        return title;
      };
      const getDescriptionEditor = (): HTMLTextAreaElement => {
        const description = screen.getByTestId(
          'commit-info-description-field',
        ) as HTMLTextAreaElement;
        expect(description).toBeInTheDocument();
        return description;
      };

      const clickToEditTitle = () => {
        act(() => {
          const title = screen.getByTestId('commit-info-rendered-title');
          expect(title).toBeInTheDocument();
          fireEvent.click(title);
        });
      };
      const clickToEditDescription = () => {
        act(() => {
          const description = screen.getByTestId('commit-info-rendered-description');
          expect(description).toBeInTheDocument();
          fireEvent.click(description);
        });
      };

      const expectIsEditingTitle = () => {
        const titleEditor = screen.queryByTestId('commit-info-title-field') as HTMLInputElement;
        expect(titleEditor).toBeInTheDocument();
      };
      const expectIsNOTEditingTitle = () => {
        const titleEditor = screen.queryByTestId('commit-info-title-field') as HTMLInputElement;
        expect(titleEditor).not.toBeInTheDocument();
      };

      const expectIsEditingDescription = () => {
        const descriptionEditor = screen.queryByTestId(
          'commit-info-description-field',
        ) as HTMLTextAreaElement;
        expect(descriptionEditor).toBeInTheDocument();
      };
      const expectIsNOTEditingDescription = () => {
        const descriptionEditor = screen.queryByTestId(
          'commit-info-description-field',
        ) as HTMLTextAreaElement;
        expect(descriptionEditor).not.toBeInTheDocument();
      };

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

        const cancelButton: HTMLButtonElement | null = within(
          screen.getByTestId('commit-info-view'),
        ).queryByText('Cancel');
        expect(cancelButton).toBeInTheDocument();

        act(() => {
          fireEvent.click(cancelButton!);
        });

        expectIsNOTEditingTitle();
        expectIsNOTEditingDescription();
      });

      it('amend button stops editing', () => {
        clickToEditTitle();
        clickToEditDescription();
        expectIsEditingTitle();
        expectIsEditingDescription();

        const amendButton: HTMLButtonElement | null = within(
          screen.getByTestId('commit-info-actions-bar'),
        ).queryByText('Amend');
        expect(amendButton).toBeInTheDocument();
        expect(amendButton?.disabled).not.toBe(true);

        act(() => {
          fireEvent.click(amendButton!);
        });

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
          expect(getDescriptionEditor().value).toBe('Summary: stacked commit\nhello new text');
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
            const titleEditor: HTMLInputElement = screen.getByTestId('commit-info-title-field');
            expect(titleEditor).toHaveFocus();
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
          expect(
            within(screen.getByTestId('commit-info-view')).queryByText('You are here'),
          ).toBeInTheDocument();
        });

        it('does not have "You are here" on non-head commit', () => {
          clickToSelectCommit('a');
          expect(
            within(screen.getByTestId('commit-info-view')).queryByText('You are here'),
          ).not.toBeInTheDocument();
        });

        it('does not have "You are here" in commit mode', () => {
          clickCommitMode();
          expect(
            within(screen.getByTestId('commit-info-view')).queryByText('You are here'),
          ).not.toBeInTheDocument();
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

          it('runs metaedit', () => {
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

            expectMessageSentToServer({
              type: 'runOperation',
              operation: {
                args: [
                  'metaedit',
                  '--rev',
                  SucceedableRevset('a'),
                  '--message',
                  'My Commit hello new title\nSummary: First commit in the stack\nhello new text',
                ],
                id: expect.anything(),
                runner: CommandRunner.Sapling,
              },
            });
          });
        });

        describe('amend', () => {
          it('runs amend with changed message', () => {
            clickToEditTitle();
            clickToEditDescription();

            {
              act(() => {
                userEvent.type(getTitleEditor(), ' hello new title');
                userEvent.type(getDescriptionEditor(), '\nhello new text');
              });
            }

            clickAmendButton();

            expectMessageSentToServer({
              type: 'runOperation',
              operation: {
                args: [
                  'amend',
                  '--message',
                  'Head Commit hello new title\nSummary: stacked commit\nhello new text',
                ],
                id: expect.anything(),
                runner: CommandRunner.Sapling,
              },
            });
          });

          it('deselects head when running amend', () => {
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
            expect(screen.queryByTestId('selected-commit')).not.toBeInTheDocument();
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

        it('runs commit with message', () => {
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

          expectMessageSentToServer({
            type: 'runOperation',
            operation: {
              args: ['commit', '--message', 'new commit title\nmy description'],
              id: expect.anything(),
              runner: CommandRunner.Sapling,
            },
          });
        });

        it('resets to amend mode after committing', () => {
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

          const commitButtonAfter = within(
            screen.getByTestId('commit-info-actions-bar'),
          ).queryByText('Commit');
          expect(commitButtonAfter).not.toBeInTheDocument();
        });

        it('does not have cancel button', () => {
          clickCommitMode();

          const cancelButton: HTMLButtonElement | null = within(
            screen.getByTestId('commit-info-view'),
          ).queryByText('Cancel');
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
              template: '[isl]\nSummary:\nTest Plan:\n',
            });
          });

          expect(getTitleEditor().value).toBe('[isl]');
          expect(getDescriptionEditor().value).toBe('Summary:\nTest Plan:\n');
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
            expect(getDescriptionEditor().value).toBe('Summary: stacked commitW');
          });
          expect(confirmSpy).toHaveBeenCalled();
        });

        it('does not confirm when clearing for amend', async () => {
          clickToEditDescription();
          const confirmSpy = jest.spyOn(platform, 'confirm');

          act(() => {
            userEvent.type(getDescriptionEditor(), 'W');
          });

          clickAmendButton();

          await waitFor(() => {
            expectIsNOTEditingTitle();
            expectIsNOTEditingDescription();
          });
          expect(confirmSpy).not.toHaveBeenCalled();
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
          const commitInfoView = screen.getByTestId('commit-info-view');
          expect(within(commitInfoView).queryByText('My Commit')).toBeInTheDocument();
          expect(within(commitInfoView).queryByText('You are here')).toBeInTheDocument();
        });

        it('takes previews into account when rendering non-head commit', () => {
          clickToSelectCommit('b'); // explicitly select, so we show even while goto runs
          clickGotoCommit('a');

          const commitInfoView = screen.getByTestId('commit-info-view');
          // we still show the other commit
          expect(within(commitInfoView).queryByText('Head Commit')).toBeInTheDocument();
          // but its not the head commit anymore, according to optimistic state
          expect(within(commitInfoView).queryByText('You are here')).not.toBeInTheDocument();
        });

        it('renders metaedit operation smoothly', () => {
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

          expectIsNOTEditingTitle();
          expectIsNOTEditingDescription();

          const commitInfoView = screen.getByTestId('commit-info-view');
          expect(within(commitInfoView).getByText('My Commit with change!')).toBeInTheDocument();
          expect(
            within(commitInfoView).getByText('Summary: First commit in the stack\nmore stuff!', {
              collapseWhitespace: false,
            }),
          ).toBeInTheDocument();
        });

        it('renders commit operation smoothly', () => {
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

          expectIsNOTEditingTitle();
          expectIsNOTEditingDescription();

          const commitInfoView = screen.getByTestId('commit-info-view');
          expect(within(commitInfoView).queryByText('New Commit')).toBeInTheDocument();
          expect(within(commitInfoView).queryByText('Message!')).toBeInTheDocument();
          expect(within(commitInfoView).queryByText('You are here')).toBeInTheDocument();

          // finish commit operation with hg log
          act(() => {
            simulateCommits({
              value: [
                COMMIT('1', 'some public base', '0', {phase: 'public'}),
                COMMIT('a', 'My Commit', '1'),
                COMMIT('b', 'Head Commit', 'a'),
                COMMIT('c', 'New Commit', 'b', {isHead: true, description: 'Message!'}),
              ],
            });
          });
          expect(within(commitInfoView).queryByText('New Commit')).toBeInTheDocument();
          expect(within(commitInfoView).queryByText('Message!')).toBeInTheDocument();
          expect(within(commitInfoView).queryByText('You are here')).toBeInTheDocument();
        });

        it('doesnt let you edit on optimistic commit', () => {
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
                COMMIT('c', 'New Commit', 'b', {isHead: true, description: 'Message!'}),
              ],
            });
          });

          clickToEditTitle();
          clickToEditDescription();
          // now you can edit just fine
          expectIsEditingTitle();
          expectIsEditingDescription();
        });

        it('renders amend operation smoothly', () => {
          clickToEditTitle();
          clickToEditDescription();
          act(() => {
            userEvent.type(getTitleEditor(), ' Hey');
            userEvent.type(getDescriptionEditor(), '\nHello');
          });

          clickAmendButton();

          // optimistic state should now be rendered, so we update the head commit
          // but not in editing mode anymore

          expectIsNOTEditingTitle();
          expectIsNOTEditingDescription();

          const commitInfoView = screen.getByTestId('commit-info-view');
          expect(within(commitInfoView).getByText('Head Commit Hey')).toBeInTheDocument();
          expect(
            within(commitInfoView).getByText('Summary: stacked commit\nHello', {
              collapseWhitespace: false,
            }),
          ).toBeInTheDocument();
          expect(within(commitInfoView).getByText('You are here')).toBeInTheDocument();

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
          expect(within(commitInfoView).getByText('Head Commit Hey')).toBeInTheDocument();
          expect(
            within(commitInfoView).getByText('Summary: stacked commit\nHello', {
              collapseWhitespace: false,
            }),
          ).toBeInTheDocument();
          expect(within(commitInfoView).getByText('You are here')).toBeInTheDocument();
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
            expect(
              within(screen.getByTestId('commit-info-view')).queryByText('Head Commit'),
            ).toBeInTheDocument();
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
              const titleEditor: HTMLInputElement = screen.getByTestId('commit-info-title-field');
              expect(titleEditor).toHaveFocus();
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
              const titleEditor: HTMLInputElement = screen.getByTestId('commit-info-title-field');
              expect(titleEditor).toHaveFocus();
            });
          });

          it('focuses fields even if amend fields already being edited', async () => {
            await clickAmendAs();

            await waitFor(() => {
              const titleEditor: HTMLInputElement = screen.getByTestId('commit-info-title-field');
              expect(titleEditor).toHaveFocus();
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
            const titleEditor: HTMLInputElement = screen.getByTestId('commit-info-title-field');
            expect(titleEditor).toHaveFocus();
          });
        });
      });
    });
  });
});
