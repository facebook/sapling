/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hash} from './types';

import {act, fireEvent, screen, waitFor, within} from '@testing-library/react';
import {nullthrows} from 'shared/utils';
import {commitMessageFieldsSchema} from './CommitInfoView/CommitMessageFields';
import {OSSCommitMessageFieldSchema} from './CommitInfoView/OSSCommitMessageFieldsSchema';
import {convertFieldNameToKey} from './CommitInfoView/utils';
import {readAtom} from './jotaiUtils';
import {individualToggleKey} from './selection';
import {expectMessageSentToServer} from './testUtils';
import {assert} from './utils';

export const CommitTreeListTestUtils = {
  withinCommitTree() {
    return within(screen.getByTestId('commit-tree-root'));
  },

  async clickGoto(commit: Hash) {
    const myCommit = screen.queryByTestId(`commit-${commit}`);
    const gotoButton = myCommit?.querySelector('.goto-button button');
    expect(gotoButton).toBeDefined();
    await act(async () => {
      await fireEvent.click(gotoButton as Element);
    });
  },
};

export const CommitInfoTestUtils = {
  withinCommitInfo() {
    return within(screen.getByTestId('commit-info-view'));
  },

  withinCommitActionBar() {
    return within(screen.getByTestId('commit-info-actions-bar'));
  },

  openCommitInfoSidebar() {
    screen.queryAllByTestId('drawer-label').forEach(el => {
      const commitInfoTab = within(el).queryByText('Commit Info');
      commitInfoTab?.click();
    });
  },

  clickToSelectCommit(hash: string, cmdClick?: boolean) {
    const commit = within(screen.getByTestId(`commit-${hash}`)).queryByTestId('draggable-commit');
    expect(commit).toBeInTheDocument();
    act(() => {
      fireEvent.click(nullthrows(commit), {[individualToggleKey]: cmdClick === true});
    });
  },

  clickCommitMode() {
    const commitRadioChoice = within(screen.getByTestId('commit-info-toolbar-top')).getByText(
      'Commit',
    );
    act(() => {
      fireEvent.click(commitRadioChoice);
    });
  },

  clickAmendMode() {
    const commitRadioChoice = within(screen.getByTestId('commit-info-toolbar-top')).getByText(
      'Amend',
    );
    act(() => {
      fireEvent.click(commitRadioChoice);
    });
  },

  async clickAmendButton() {
    const amendButton: HTMLButtonElement | null = within(
      screen.getByTestId('commit-info-actions-bar'),
    ).queryByText('Amend');
    expect(amendButton).toBeInTheDocument();
    act(() => {
      fireEvent.click(nullthrows(amendButton));
    });
    await waitFor(() =>
      expectMessageSentToServer({
        type: 'runOperation',
        operation: expect.objectContaining({
          args: expect.arrayContaining(['amend']),
        }),
      }),
    );
  },

  clickAmendMessageButton() {
    const amendMessageButton: HTMLButtonElement | null = within(
      screen.getByTestId('commit-info-actions-bar'),
    ).queryByText('Amend Message');
    expect(amendMessageButton).toBeInTheDocument();
    act(() => {
      fireEvent.click(nullthrows(amendMessageButton));
    });
  },

  async clickCommitButton() {
    const commitButton: HTMLButtonElement | null = within(
      screen.getByTestId('commit-info-actions-bar'),
    ).queryByText('Commit');
    expect(commitButton).toBeInTheDocument();
    act(() => {
      fireEvent.click(nullthrows(commitButton));
    });
    await waitFor(() =>
      expectMessageSentToServer({
        type: 'runOperation',
        operation: expect.objectContaining({
          args: expect.arrayContaining(['commit']),
        }),
      }),
    );
  },

  clickCancel() {
    const cancelButton: HTMLButtonElement | null =
      CommitInfoTestUtils.withinCommitInfo().queryByText('Cancel');
    expect(cancelButton).toBeInTheDocument();

    act(() => {
      fireEvent.click(nullthrows(cancelButton));
    });
  },

  /** Get the textarea for the title editor */
  getTitleEditor(): HTMLTextAreaElement {
    const title = screen.getByTestId('commit-info-title-field') as HTMLTextAreaElement;
    expect(title).toBeInTheDocument();
    return title;
  },

  /** Get the textarea for the description editor
   * For internal builds, this points to the "summary" editor instead of the "description" editor
   */
  getDescriptionEditor(): HTMLTextAreaElement {
    const description = screen.getByTestId(
      isInternalMessageFields() ? 'commit-info-summary-field' : 'commit-info-description-field',
    ) as HTMLTextAreaElement;
    expect(description).toBeInTheDocument();
    return description;
  },

  /** Get the textarea for the test plan editor. Unavailable in OSS tests (use internal-only tests). */
  getTestPlanEditor(): HTMLTextAreaElement {
    assert(isInternalMessageFields(), 'Cannot edit test plan in OSS');
    const testPlan = screen.getByTestId('commit-info-test-plan-field') as HTMLTextAreaElement;
    expect(testPlan).toBeInTheDocument();
    return testPlan;
  },

  /** Get the input element for a given field's editor, according to the field key in the FieldConfig (actually just a div in tests) */
  getFieldEditor(key: string): HTMLDivElement {
    const renderKey = convertFieldNameToKey(key);
    const el = screen.getByTestId(`commit-info-${renderKey}-field`) as HTMLDivElement;
    expect(el).toBeInTheDocument();
    return el;
  },

  descriptionTextContent() {
    return CommitInfoTestUtils.getDescriptionEditor().value;
  },

  clickToEditTitle() {
    act(() => {
      const title = screen.getByTestId('commit-info-rendered-title');
      expect(title).toBeInTheDocument();
      fireEvent.click(title);
    });
  },
  clickToEditDescription() {
    act(() => {
      const description = screen.getByTestId(
        isInternalMessageFields()
          ? 'commit-info-rendered-summary'
          : 'commit-info-rendered-description',
      );
      expect(description).toBeInTheDocument();
      fireEvent.click(description);
    });
  },
  clickToEditTestPlan() {
    assert(isInternalMessageFields(), 'Cannot edit test plan in OSS');
    act(() => {
      const testPlan = screen.getByTestId('commit-info-rendered-test-plan');
      expect(testPlan).toBeInTheDocument();
      fireEvent.click(testPlan);
    });
  },

  /** Internal tests only, since GitHub's message schema does not include this field */
  clickToEditReviewers() {
    act(() => {
      const title = screen.getByTestId('commit-info-rendered-reviewers');
      expect(title).toBeInTheDocument();
      fireEvent.click(title);
    });
  },

  expectIsEditingTitle() {
    const titleEditor = screen.queryByTestId('commit-info-title-field') as HTMLInputElement;
    expect(titleEditor).toBeInTheDocument();
  },
  expectIsNOTEditingTitle() {
    const titleEditor = screen.queryByTestId('commit-info-title-field') as HTMLInputElement;
    expect(titleEditor).not.toBeInTheDocument();
  },

  expectIsEditingDescription() {
    const descriptionEditor = screen.queryByTestId(
      isInternalMessageFields() ? 'commit-info-summary-field' : 'commit-info-description-field',
    ) as HTMLTextAreaElement;
    expect(descriptionEditor).toBeInTheDocument();
  },
  expectIsNOTEditingDescription() {
    const descriptionEditor = screen.queryByTestId(
      isInternalMessageFields() ? 'commit-info-summary-field' : 'commit-info-description-field',
    ) as HTMLTextAreaElement;
    expect(descriptionEditor).not.toBeInTheDocument();
  },
};

export const MergeConflictTestUtils = {
  waitForContinueButtonNotDisabled: () =>
    waitFor(() =>
      expect(
        within(screen.getByTestId('commit-tree-root')).getByTestId(
          'conflict-continue-button',
        ) as HTMLButtonElement,
      ).not.toBeDisabled(),
    ),
  clickContinueConflicts: () =>
    act(() => {
      fireEvent.click(
        within(screen.getByTestId('commit-tree-root')).getByTestId('conflict-continue-button'),
      );
    }),
  expectInMergeConflicts: () =>
    expect(
      within(screen.getByTestId('commit-tree-root')).getByText('Unresolved Merge Conflicts'),
    ).toBeInTheDocument(),
  expectNotInMergeConflicts: () =>
    expect(
      within(screen.getByTestId('commit-tree-root')).queryByText('Unresolved Merge Conflicts'),
    ).not.toBeInTheDocument(),
};

function isInternalMessageFields(): boolean {
  const schema = readAtom(commitMessageFieldsSchema);
  return schema !== OSSCommitMessageFieldSchema;
}

/**
 * When querying changed files, there may be unicode left-to-right marks in the path,
 * which make the test hard to read. This util searches for a string, inserting optional
 * RTL marks at path boundaries.
 */
export function ignoreRTL(s: string): RegExp {
  return new RegExp(`^\u200E?${s}\u200E?$`);
}
