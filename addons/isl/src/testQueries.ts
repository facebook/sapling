/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hash} from './types';

import {
  commitMessageFieldsSchema,
  OSSDefaultFieldSchema,
} from './CommitInfoView/CommitMessageFields';
import {screen, within, fireEvent, waitFor} from '@testing-library/react';
import {act} from 'react-dom/test-utils';
import {snapshot_UNSTABLE} from 'recoil';
import {unwrap} from 'shared/utils';

export const CommitTreeListTestUtils = {
  withinCommitTree() {
    return within(screen.getByTestId('commit-tree-root'));
  },

  clickGoto(commit: Hash) {
    const myCommit = screen.queryByTestId(`commit-${commit}`);
    const gotoButton = myCommit?.querySelector('.goto-button button');
    expect(gotoButton).toBeDefined();
    fireEvent.click(gotoButton as Element);
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
      fireEvent.click(unwrap(commit), {metaKey: cmdClick === true});
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

  clickAmendButton() {
    const amendButton: HTMLButtonElement | null = within(
      screen.getByTestId('commit-info-actions-bar'),
    ).queryByText('Amend');
    expect(amendButton).toBeInTheDocument();
    act(() => {
      fireEvent.click(unwrap(amendButton));
    });
  },

  clickAmendMessageButton() {
    const amendMessageButton: HTMLButtonElement | null = within(
      screen.getByTestId('commit-info-actions-bar'),
    ).queryByText('Amend Message');
    expect(amendMessageButton).toBeInTheDocument();
    act(() => {
      fireEvent.click(unwrap(amendMessageButton));
    });
  },

  clickCommitButton() {
    const commitButton: HTMLButtonElement | null = within(
      screen.getByTestId('commit-info-actions-bar'),
    ).queryByText('Commit');
    expect(commitButton).toBeInTheDocument();
    act(() => {
      fireEvent.click(unwrap(commitButton));
    });
  },

  clickCancel() {
    const cancelButton: HTMLButtonElement | null =
      CommitInfoTestUtils.withinCommitInfo().queryByText('Cancel');
    expect(cancelButton).toBeInTheDocument();

    act(() => {
      fireEvent.click(unwrap(cancelButton));
    });
  },

  /** Get the outer custom element for the title editor (actually just a div in tests) */
  getTitleWrapper(): HTMLDivElement {
    const title = screen.getByTestId('commit-info-title-field') as HTMLDivElement;
    expect(title).toBeInTheDocument();
    return title;
  },
  /** Get the inner textarea for the title editor (inside the fake shadow dom) */
  getTitleEditor(): HTMLTextAreaElement {
    const textarea = CommitInfoTestUtils.getTitleWrapper();
    return (textarea as unknown as {control: HTMLTextAreaElement}).control;
  },

  /** Get the outer custom element for the description editor (actually just a div in tests)
   * For internal builds, this points to the "summary" editor instead of the "description" editor
   */
  getDescriptionWrapper(): HTMLDivElement {
    const description = screen.getByTestId(
      isInternalMessageFields() ? 'commit-info-summary-field' : 'commit-info-description-field',
    ) as HTMLDivElement;
    expect(description).toBeInTheDocument();
    return description;
  },
  /** Get the inner textarea for the description editor (inside the fake shadow dom)
   * For internal builds, this points to the "summary" editor instead of the "description" editor
   */
  getDescriptionEditor(): HTMLTextAreaElement {
    const textarea = CommitInfoTestUtils.getDescriptionWrapper();
    return (textarea as unknown as {control: HTMLTextAreaElement}).control;
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
  const snapshot = snapshot_UNSTABLE();
  const schema = snapshot.getLoadable(commitMessageFieldsSchema).valueOrThrow();
  return schema !== OSSDefaultFieldSchema;
}

/**
 * When querying changed files, there may be unicode left-to-right marks in the path,
 * which make the test hard to read. This util searches for a string, inserting optional
 * RTL marks at path boundaries.
 */
export function ignoreRTL(s: string): RegExp {
  return new RegExp(`^\u200E?${s}\u200E?$`);
}
