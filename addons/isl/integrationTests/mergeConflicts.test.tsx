/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {act, screen, waitFor, within} from '@testing-library/react';
import {initRepo} from './setup';

describe('merge conflicts integration test', () => {
  it('shows conflicts, supports resolving, and continuing the operation', async () => {
    const {cleanup, sl, drawdag, writeFileInRepo, refresh} = await initRepo();
    const {ignoreRTL} = await import('../src/testQueries');
    await act(() =>
      drawdag(`
        C
        |
        B
        |
        A
        .
python:
commit('C', files={"file1.txt": "bar\\n"})
commit('B', files={"file1.txt": "foo\\n"})
commit('A', files={"file1.txt": "base\\n"})
      `),
    );

    await act(async () => {
      await sl(['goto', 'desc(B)']);
      await writeFileInRepo('file1.txt', 'conflict');
      // this amend onto B will hit conflicts with C
      await sl(['amend', '--rebase']).catch(() => undefined);
    });
    refresh();

    await waitFor(() =>
      within(screen.getByTestId('commit-tree-root')).getByText('Unresolved Merge Conflicts'),
    );
    await waitFor(() =>
      within(screen.getByTestId('commit-tree-root')).getByText(ignoreRTL('file1.txt')),
    );

    await act(cleanup);
  });
});
