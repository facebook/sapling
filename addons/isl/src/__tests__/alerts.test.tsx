/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import App from '../App';
import {
  closeCommitInfoSidebar,
  expectMessageSentToServer,
  simulateCommits,
  COMMIT,
  simulateMessageFromServer,
  simulateRepoConnected,
} from '../testUtils';
import {render, screen, fireEvent} from '@testing-library/react';
import {act} from 'react-dom/test-utils';

describe('cwd', () => {
  beforeEach(() => {
    render(<App />);
    act(() => {
      simulateRepoConnected();
      closeCommitInfoSidebar();
      expectMessageSentToServer({
        type: 'subscribe',
        kind: 'smartlogCommits',
        subscriptionID: expect.anything(),
      });
      simulateCommits({
        value: [
          COMMIT('1', 'some public base', '0', {phase: 'public'}),
          COMMIT('a', 'My Commit', '1'),
          COMMIT('b', 'Another Commit', 'a', {isHead: true}),
        ],
      });
    });
  });

  it('fetches alerts', () => {
    expectMessageSentToServer({type: 'fetchActiveAlerts'});
  });

  it('shows alerts', () => {
    expectMessageSentToServer({type: 'fetchActiveAlerts'});
    act(() => {
      simulateMessageFromServer({
        type: 'fetchedActiveAlerts',
        alerts: [
          {
            key: 'test',
            title: 'Test Alert',
            description: 'This is a test',
            severity: 'SEV 4',
            url: 'https://sapling-scm.com',
            ['show-in-isl']: true,
          },
        ],
      });
    });
    expect(screen.getByText('Test Alert')).toBeInTheDocument();
    expect(screen.getByText('This is a test')).toBeInTheDocument();
    expect(screen.getByText('SEV 4')).toBeInTheDocument();
  });

  it('dismiss alerts', () => {
    expectMessageSentToServer({type: 'fetchActiveAlerts'});
    act(() => {
      simulateMessageFromServer({
        type: 'fetchedActiveAlerts',
        alerts: [
          {
            key: 'test-dismiss',
            title: 'Test Alert',
            description: 'This is a test',
            severity: 'SEV 4',
            url: 'https://sapling-scm.com',
            ['show-in-isl']: true,
          },
        ],
      });
    });

    expect(screen.getByText('Test Alert')).toBeInTheDocument();
    act(() => {
      fireEvent.click(screen.getByTestId('dismiss-alert'));
    });
    const found = localStorage.getItem('isl.dismissed-alerts');
    expect(found).toEqual(JSON.stringify({'test-dismiss': true}));

    expect(screen.queryByText('Test Alert')).not.toBeInTheDocument();
  });
});
