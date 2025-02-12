/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {act, screen, waitFor, within} from '@testing-library/react';
import {initRepo} from './setup';

describe('multiple merge conflicts integration test', () => {
  it('shows conflicts, supports resolving, and continuing the operation', async () => {
    const {cleanup, sl, drawdag, writeFileInRepo, refresh} = await initRepo();

    const {MergeConflictTestUtils, ignoreRTL} = await import('../src/testQueries');
    const {
      expectInMergeConflicts,
      expectNotInMergeConflicts,
      waitForContinueButtonNotDisabled,
      clickContinueConflicts,
    } = MergeConflictTestUtils;

    await act(() =>
      drawdag(`
        C
        |
        B
        |
        A
        .
python:
commit('C', files={"file2.txt": "baz\\n"})
commit('B', files={"file1.txt": "foo\\n"})
commit('A', files={"file1.txt": "base\\n", "file2.txt": "base\\n"})
      `),
    );

    await act(async () => {
      await sl(['goto', 'desc(A)']);
      await writeFileInRepo('file1.txt', 'conflict');
      await writeFileInRepo('file2.txt', 'conflict');
      // this amend onto B will hit conflicts with C
      await sl(['amend', '--rebase']).catch(() => undefined);
    });

    await waitFor(() => expectInMergeConflicts());
    await waitFor(() =>
      within(screen.getByTestId('commit-tree-root')).getByText(ignoreRTL('file1.txt')),
    );

    await act(async () => {
      await sl(['resolve', '--tool', 'internal:union', 'file1.txt']);
    });
    refresh();

    await waitForContinueButtonNotDisabled();
    clickContinueConflicts();

    await waitFor(() =>
      within(screen.getByTestId('commit-tree-root')).getByText(ignoreRTL('file2.txt')),
    );
    await act(async () => {
      await sl(['resolve', '--tool', 'internal:union', 'file2.txt']);
    });
    refresh();

    await waitForContinueButtonNotDisabled();
    clickContinueConflicts();

    await waitFor(() => expectNotInMergeConflicts());

    await act(cleanup);
  });
});
