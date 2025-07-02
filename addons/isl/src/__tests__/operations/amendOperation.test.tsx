/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {act, render, waitFor} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import * as utils from 'shared/utils';
import App from '../../App';
import {CommitInfoTestUtils} from '../../testQueries';
import {
  COMMIT,
  expectMessageSentToServer,
  getLastMessageOfTypeSentToServer,
  openCommitInfoSidebar,
  resetTestMessages,
  simulateCommits,
  simulateMessageFromServer,
  simulateUncommittedChangedFiles,
} from '../../testUtils';

describe('AmendOperation', () => {
  beforeEach(() => {
    resetTestMessages();
    render(<App />);
    act(() => {
      expectMessageSentToServer({
        type: 'subscribe',
        kind: 'smartlogCommits',
        subscriptionID: expect.anything(),
      });
      simulateUncommittedChangedFiles({
        value: [
          {path: 'file1.txt', status: 'M'},
          {path: 'file2.txt', status: 'A'},
          {path: 'file3.txt', status: 'R'},
        ],
      });
      simulateCommits({
        value: [
          COMMIT('1', 'Commit 1', '0', {phase: 'public'}),
          COMMIT('a', 'Commit A', '1'),
          COMMIT('b', 'Commit B', 'a', {isDot: true}),
        ],
      });
    });
  });

  it('on error, restores edited commit message to try again', async () => {
    act(() => openCommitInfoSidebar());
    act(() => {
      CommitInfoTestUtils.clickToEditTitle();
      CommitInfoTestUtils.clickToEditDescription();
    });
    act(() => {
      const title = CommitInfoTestUtils.getTitleEditor();
      userEvent.type(title, 'My Commit');
      const desc = CommitInfoTestUtils.getDescriptionEditor();
      userEvent.type(desc, 'My description');
    });

    await CommitInfoTestUtils.clickAmendButton();
    const message = await waitFor(() =>
      utils.nullthrows(getLastMessageOfTypeSentToServer('runOperation')),
    );
    const id = message.operation.id;

    CommitInfoTestUtils.expectIsNOTEditingTitle();

    act(() => {
      simulateMessageFromServer({
        type: 'operationProgress',
        kind: 'exit',
        exitCode: 1,
        id,
        timestamp: 0,
      });
    });

    waitFor(() => {
      CommitInfoTestUtils.expectIsEditingTitle();
      const title = CommitInfoTestUtils.getTitleEditor();
      expect(title).toHaveValue('My Commit');
      CommitInfoTestUtils.expectIsEditingDescription();
      const desc = CommitInfoTestUtils.getDescriptionEditor();
      expect(desc).toHaveValue('My description');
    });
  });

  it('on error, merges messages when restoring edited commit message to try again', async () => {
    act(() => openCommitInfoSidebar());

    act(() => {
      CommitInfoTestUtils.clickToEditTitle();
      CommitInfoTestUtils.clickToEditDescription();
    });
    act(() => {
      const title = CommitInfoTestUtils.getTitleEditor();
      userEvent.type(title, 'My Commit');
      const desc = CommitInfoTestUtils.getDescriptionEditor();
      userEvent.type(desc, 'My description');
    });

    await CommitInfoTestUtils.clickAmendButton();
    const message = await waitFor(() =>
      utils.nullthrows(getLastMessageOfTypeSentToServer('runOperation')),
    );
    const id = message.operation.id;

    CommitInfoTestUtils.expectIsNOTEditingTitle();

    act(() => {
      openCommitInfoSidebar();
    });
    act(() => {
      CommitInfoTestUtils.clickToEditTitle();
      CommitInfoTestUtils.clickToEditDescription();
    });
    act(() => {
      const title = CommitInfoTestUtils.getTitleEditor();
      userEvent.type(title, 'other title');
      const desc = CommitInfoTestUtils.getDescriptionEditor();
      userEvent.type(desc, 'other description');
    });

    act(() => {
      simulateMessageFromServer({
        type: 'operationProgress',
        kind: 'exit',
        exitCode: 1,
        id,
        timestamp: 0,
      });
    });

    waitFor(() => {
      CommitInfoTestUtils.expectIsEditingTitle();
      const title = CommitInfoTestUtils.getTitleEditor();
      expect(title).toHaveValue('other title, My Commit');
      CommitInfoTestUtils.expectIsEditingDescription();
      const desc = CommitInfoTestUtils.getDescriptionEditor();
      expect(desc).toHaveValue('other description, My description');
    });
  });
});
