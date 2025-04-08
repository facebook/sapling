/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {act, screen, waitFor, within} from '@testing-library/react';
import {initRepo} from './setup';

describe('commits integration test', () => {
  it('shows commits', async () => {
    const {cleanup, drawdag, refresh} = await initRepo();
    await act(() =>
      drawdag(`
        C D
        |/
        B
        |
        A
      `),
    );
    refresh();

    // commits should show up in the commit tree
    await waitFor(() => within(screen.getByTestId('commit-tree-root')).getByText('A'));
    await waitFor(() => within(screen.getByTestId('commit-tree-root')).getByText('B'));
    await waitFor(() => within(screen.getByTestId('commit-tree-root')).getByText('C'));
    await waitFor(() => within(screen.getByTestId('commit-tree-root')).getByText('D'));

    await act(cleanup);
  });
});
