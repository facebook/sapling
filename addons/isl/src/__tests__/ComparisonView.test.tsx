/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import App from '../App';
import {
  closeCommitInfoSidebar,
  COMMIT,
  expectMessageSentToServer,
  resetTestMessages,
  simulateCommits,
  simulateMessageFromServer,
  simulateUncommittedChangedFiles,
} from '../testUtils';
import {act, screen, render, waitFor, fireEvent, cleanup, within} from '@testing-library/react';
import {selector, snapshot_UNSTABLE} from 'recoil';
import {ComparisonType} from 'shared/Comparison';

afterEach(cleanup);

jest.mock('../MessageBus');

const UNCOMMITTED_CHANGES_DIFF = `\
diff -r a1b2c3d4e5f6 deletedFile.txt
--- deletedFile.txt
+++ /dev/null
@@ -1,1 +0,0 @@
-Goodbye
diff -r a1b2c3d4e5f6 newFile.txt
--- /dev/null
+++ newFile.txt
@@ -0,0 +1,1 @@
+hello
diff -r a1b2c3d4e5f6 someFile.txt
--- someFile.txt
+++ someFile.txt
@@ -7,5 +7,5 @@
 line 7
 line 8
-line 9
+line 9 - modified
 line 10
 line 11
`;

/* eslint-disable @typescript-eslint/no-non-null-assertion */

const clearSelectorCachesState = selector({
  key: 'clearSelectorCachesState',
  get: ({getCallback}) =>
    getCallback(({snapshot, refresh}) => () => {
      for (const node of snapshot.getNodes_UNSTABLE()) {
        refresh(node);
      }
    }),
});

// reset recoil caches between test runs
afterEach(() => {
  const clearSelectorCaches = snapshot_UNSTABLE().getLoadable(clearSelectorCachesState).getValue();
  clearSelectorCaches();
});

describe('ComparisonView', () => {
  beforeEach(() => {
    resetTestMessages();
    render(<App />);
    act(() => {
      closeCommitInfoSidebar();
      simulateCommits({
        value: [
          COMMIT('1', 'some public base', '0', {phase: 'public'}),
          COMMIT('a', 'My Commit', '1'),
          COMMIT('b', 'Another Commit', 'a', {isHead: true}),
        ],
      });
      simulateUncommittedChangedFiles({
        value: [{path: 'src/file1.txt', status: 'M'}],
      });
    });
  });

  function clickComparisonViewButton() {
    act(() => {
      const button = screen.getByTestId('open-comparison-view-button-UNCOMMITTED');
      fireEvent.click(button);
    });
  }
  async function openUncommittedChangesComparison() {
    clickComparisonViewButton();
    await waitFor(() =>
      expectMessageSentToServer({
        type: 'requestComparison',
        comparison: {type: ComparisonType.UncommittedChanges},
      }),
    );
    act(() => {
      simulateMessageFromServer({
        type: 'comparison',
        comparison: {type: ComparisonType.UncommittedChanges},
        data: {diff: {value: UNCOMMITTED_CHANGES_DIFF}},
      });
    });
  }
  function inComparisonView() {
    return within(screen.getByTestId('comparison-view'));
  }

  function closeComparisonView() {
    const closeButton = inComparisonView().getByTestId('close-comparison-view-button');
    expect(closeButton).toBeInTheDocument();
    act(() => {
      fireEvent.click(closeButton);
    });
  }

  it('Loads comparison', async () => {
    await openUncommittedChangesComparison();
  });

  it('parses files from comparison', async () => {
    await openUncommittedChangesComparison();
    expect(inComparisonView().getByText('someFile.txt')).toBeInTheDocument();
    expect(inComparisonView().getByText('newFile.txt')).toBeInTheDocument();
    expect(inComparisonView().getByText('deletedFile.txt')).toBeInTheDocument();
  });

  it('show file contents', async () => {
    await openUncommittedChangesComparison();
    expect(inComparisonView().getByText('- modified')).toBeInTheDocument();
    expect(inComparisonView().getAllByText('line 7')[0]).toBeInTheDocument();
    expect(inComparisonView().getAllByText('line 8')[0]).toBeInTheDocument();
    expect(inComparisonView().getAllByText('line 9')[0]).toBeInTheDocument();
    expect(inComparisonView().getAllByText('line 10')[0]).toBeInTheDocument();
    expect(inComparisonView().getAllByText('line 11')[0]).toBeInTheDocument();
  });

  it('loads remaining lines', async () => {
    await openUncommittedChangesComparison();
    const expandButton = inComparisonView().getByText('Expand 6 lines');
    expect(expandButton).toBeInTheDocument();
    act(() => {
      fireEvent.click(expandButton);
    });
    await waitFor(() => {
      expectMessageSentToServer({
        type: 'requestComparisonContextLines',
        id: {path: 'someFile.txt', comparison: {type: ComparisonType.UncommittedChanges}},
        numLines: 6,
        start: 1,
      });
    });
    act(() => {
      simulateMessageFromServer({
        type: 'comparisonContextLines',
        lines: ['line 1', 'line 2', 'line 3', 'line 4', 'line 5', 'line 6'],
        path: 'someFile.txt',
      });
    });
    await waitFor(() => {
      expect(inComparisonView().getAllByText('line 1')[0]).toBeInTheDocument();
      expect(inComparisonView().getAllByText('line 2')[0]).toBeInTheDocument();
      expect(inComparisonView().getAllByText('line 3')[0]).toBeInTheDocument();
      expect(inComparisonView().getAllByText('line 4')[0]).toBeInTheDocument();
      expect(inComparisonView().getAllByText('line 5')[0]).toBeInTheDocument();
      expect(inComparisonView().getAllByText('line 6')[0]).toBeInTheDocument();
    });
  });

  it('can close comparison', async () => {
    await openUncommittedChangesComparison();
    expect(inComparisonView().getByText('- modified')).toBeInTheDocument();
    closeComparisonView();
    expect(screen.queryByText('- modified')).not.toBeInTheDocument();
  });

  it('invalidates cached remaining lines when the head commit changes', async () => {
    await openUncommittedChangesComparison();
    const clickExpand = () => {
      const expandButton = inComparisonView().getByText('Expand 6 lines');
      expect(expandButton).toBeInTheDocument();
      act(() => {
        fireEvent.click(expandButton);
      });
    };
    clickExpand();
    await waitFor(() => {
      expectMessageSentToServer({
        type: 'requestComparisonContextLines',
        id: {path: 'someFile.txt', comparison: {type: ComparisonType.UncommittedChanges}},
        numLines: 6,
        start: 1,
      });
    });
    act(() => {
      simulateMessageFromServer({
        type: 'comparisonContextLines',
        lines: ['line 1', 'line 2', 'line 3', 'line 4', 'line 5', 'line 6'],
        path: 'someFile.txt',
      });
    });
    await waitFor(() => {
      expect(inComparisonView().getAllByText('line 1')[0]).toBeInTheDocument();
      expect(inComparisonView().getAllByText('line 6')[0]).toBeInTheDocument();
    });

    closeComparisonView();
    resetTestMessages(); // make sure we don't find previous "requestComparisonContextLines" in later assertions

    // head commit changes

    act(() => {
      simulateCommits({
        value: [
          COMMIT('1', 'some public base', '0', {phase: 'public'}),
          COMMIT('a', 'My Commit', '1'),
          COMMIT('b', 'Another Commit', 'a'),
          COMMIT('c', 'New commit!', 'b', {isHead: true}),
        ],
      });
    });
    await openUncommittedChangesComparison();
    expect(inComparisonView().getByText('- modified')).toBeInTheDocument();

    clickExpand();

    // previous context lines are no longer there
    expect(inComparisonView().queryByText('line 1')).not.toBeInTheDocument();

    // it should ask for the line contents from the server again
    await waitFor(() => {
      expectMessageSentToServer({
        type: 'requestComparisonContextLines',
        id: {path: 'someFile.txt', comparison: {type: ComparisonType.UncommittedChanges}},
        numLines: 6,
        start: 1,
      });
    });
    act(() => {
      simulateMessageFromServer({
        type: 'comparisonContextLines',
        lines: [
          'different line 1',
          'different line 2',
          'different line 3',
          'different line 4',
          'different line 5',
          'different line 6',
        ],
        path: 'someFile.txt',
      });
    });
    // new data is used
    await waitFor(() => {
      expect(inComparisonView().getAllByText('different line 1')[0]).toBeInTheDocument();
      expect(inComparisonView().getAllByText('different line 6')[0]).toBeInTheDocument();
    });
  });

  it('refresh button requests new data', async () => {
    await openUncommittedChangesComparison();
    resetTestMessages();

    act(() => {
      fireEvent.click(inComparisonView().getByTestId('comparison-refresh-button'));
    });

    expectMessageSentToServer({
      type: 'requestComparison',
      comparison: {type: ComparisonType.UncommittedChanges},
    });
  });

  it('changing comparison mode requests new data', async () => {
    await openUncommittedChangesComparison();

    act(() => {
      fireEvent.change(inComparisonView().getByTestId('comparison-view-picker'), {
        target: {value: ComparisonType.StackChanges},
      });
    });
    expectMessageSentToServer({
      type: 'requestComparison',
      comparison: {type: ComparisonType.StackChanges},
    });
  });

  it('shows a spinner while a fetch is ongoing', () => {
    clickComparisonViewButton();
    expect(inComparisonView().getByTestId('comparison-loading')).toBeInTheDocument();

    act(() => {
      simulateMessageFromServer({
        type: 'comparison',
        comparison: {type: ComparisonType.UncommittedChanges},
        data: {diff: {value: UNCOMMITTED_CHANGES_DIFF}},
      });
    });
    expect(inComparisonView().queryByTestId('comparison-loading')).not.toBeInTheDocument();
  });
});
