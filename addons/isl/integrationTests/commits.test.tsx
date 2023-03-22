/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {initRepo} from './setup';
import {act, screen, waitFor, within} from '@testing-library/react';

describe('commits integration test', () => {
  it('shows commits', async () => {
    const {sl, cleanup, writeFileInRepo} = await initRepo();

    await act(async () => {
      await writeFileInRepo('file.txt', 'hello, world!');
    });

    // changed file should appear as uncommitted change
    await waitFor(() => within(screen.getByTestId('commit-tree-root')).getByText('file.txt'));

    await act(async () => {
      await sl(['commit', '-m', 'Commit A']);
    });

    // new commit should show up in the commit tree
    await waitFor(() => within(screen.getByTestId('commit-tree-root')).getByText('Commit A'));

    await cleanup();
  });
});
