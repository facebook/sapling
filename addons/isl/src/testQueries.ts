/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hash} from './types';

import {screen, within, fireEvent, waitFor} from '@testing-library/react';
import {act} from 'react-dom/test-utils';
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

  clickToSelectCommit(hash: string) {
    const commit = within(screen.getByTestId(`commit-${hash}`)).queryByTestId('draggable-commit');
    expect(commit).toBeInTheDocument();
    act(() => {
      fireEvent.click(unwrap(commit));
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

  /** Get the outer custom element for the description editor (actually just a div in tests) */
  getDescriptionWrapper(): HTMLDivElement {
    const description = screen.getByTestId('commit-info-description-field') as HTMLDivElement;
    expect(description).toBeInTheDocument();
    return description;
  },
  /** Get the inner textarea for the description editor (inside the fake shadow dom) */
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
      const description = screen.getByTestId('commit-info-rendered-description');
      expect(description).toBeInTheDocument();
      fireEvent.click(description);
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
      'commit-info-description-field',
    ) as HTMLTextAreaElement;
    expect(descriptionEditor).toBeInTheDocument();
  },
  expectIsNOTEditingDescription() {
    const descriptionEditor = screen.queryByTestId(
      'commit-info-description-field',
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
