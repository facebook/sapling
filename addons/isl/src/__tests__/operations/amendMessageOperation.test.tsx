/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import App from '../../App';
import {CommitInfoTestUtils} from '../../testQueries';
import {
  resetTestMessages,
  expectMessageSentToServer,
  simulateCommits,
  COMMIT,
  simulateUncommittedChangedFiles,
  simulateMessageFromServer,
  openCommitInfoSidebar,
} from '../../testUtils';
import {render, waitFor, act} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import * as utils from 'shared/utils';

describe('AmendMessageOperation', () => {
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

  it('on error, restores edited commit message to try again', () => {
    act(() => CommitInfoTestUtils.clickToSelectCommit('a'));
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

    jest.spyOn(utils, 'randomId').mockImplementationOnce(() => '1111');
    act(() => {
      CommitInfoTestUtils.clickAmendMessageButton();
    });

    CommitInfoTestUtils.expectIsNOTEditingTitle();

    act(() => CommitInfoTestUtils.clickToSelectCommit('b'));
    expect(CommitInfoTestUtils.withinCommitInfo().getByText('You are here')).toBeInTheDocument();

    act(() => {
      simulateMessageFromServer({
        type: 'operationProgress',
        kind: 'exit',
        exitCode: 1,
        id: '1111',
        timestamp: 0,
      });
    });

    waitFor(() => {
      expect(
        CommitInfoTestUtils.withinCommitInfo().getByText('You are here'),
      ).not.toBeInTheDocument();
      CommitInfoTestUtils.expectIsEditingTitle();
      const title = CommitInfoTestUtils.getTitleEditor();
      expect(title).toHaveValue('My Commit');
      CommitInfoTestUtils.expectIsEditingDescription();
      const desc = CommitInfoTestUtils.getDescriptionEditor();
      expect(desc).toHaveValue('My description');
    });
  });
});
