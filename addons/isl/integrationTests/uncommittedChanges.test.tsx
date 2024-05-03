/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {initRepo} from './setup';
import {act, screen, waitFor, within} from '@testing-library/react';

describe('uncommitted changes integration test', () => {
  it('shows changed file', async () => {
    const {cleanup, writeFileInRepo} = await initRepo();
    const {ignoreRTL} = await import('../src/testQueries');
    await act(async () => {
      await writeFileInRepo('file.txt', 'hello, world!');
    });

    // changed file should appear as uncommitted change
    await waitFor(() =>
      within(screen.getByTestId('commit-tree-root')).getByText(ignoreRTL('file.txt')),
    );

    await act(cleanup);
  });
});
