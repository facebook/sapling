/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import App from '../App';
import {
  resetTestMessages,
  closeCommitInfoSidebar,
  simulateCommits,
  COMMIT,
  dragAndDropCommits,
} from '../testUtils';
import {screen, act, render, fireEvent} from '@testing-library/react';

describe('focus mode', () => {
  const COMMITS = [
    COMMIT('3', 'another public branch', '0', {
      phase: 'public',
      remoteBookmarks: ['remote/stable'],
    }),
    COMMIT('y', 'Commit Y', 'x'),
    COMMIT('x', 'Commit X', '2'),
    COMMIT('c2', 'Commit C2', '2', {closestPredecessors: ['c']}),
    COMMIT('2', 'another public branch', '0', {
      phase: 'public',
      remoteBookmarks: ['remote/master'],
    }),
    COMMIT('f', 'Commit F', 'e'), // after `.` still incldued
    COMMIT('e', 'Commit E', 'd', {isDot: true}),
    COMMIT('d', 'Commit D', 'c'), // branch
    COMMIT('c', 'Commit C', 'b'),
    COMMIT('b2', 'Commit B2', 'a', {closestPredecessors: ['b']}), // succeeded within this branch
    COMMIT('b', 'Commit B', 'a'),
    COMMIT('a', 'Commit A', '1'),
    COMMIT('1', 'some public base', '0', {phase: 'public'}),
  ];

  beforeEach(() => {
    resetTestMessages();
    render(<App />);

    act(() => {
      closeCommitInfoSidebar();
      simulateCommits({value: COMMITS});
    });
  });

  const toggleFocusMode = () => {
    const toggle = screen.getByTestId('focus-mode-toggle');
    fireEvent.click(toggle);
  };

  it('can set focus mode', () => {
    toggleFocusMode();
    expect(screen.getByTestId('focus-mode-toggle').dataset.focusMode).toEqual('true');
    toggleFocusMode();
    expect(screen.getByTestId('focus-mode-toggle').dataset.focusMode).toEqual('false');
  });

  it('hides commits outside your stack', () => {
    expect(screen.getByText('remote/master')).toBeInTheDocument();
    expect(screen.getByText('remote/stable')).toBeInTheDocument();
    expect(screen.getByText('Commit A')).toBeInTheDocument();
    expect(screen.getByText('Commit B')).toBeInTheDocument();
    expect(screen.getByText('Commit B2')).toBeInTheDocument();
    expect(screen.getByText('Commit C')).toBeInTheDocument();
    expect(screen.getByText('Commit C2')).toBeInTheDocument();
    expect(screen.getByText('Commit D')).toBeInTheDocument();
    expect(screen.getByText('Commit E')).toBeInTheDocument();
    expect(screen.getByText('Commit F')).toBeInTheDocument();

    expect(screen.getByText('Commit X')).toBeInTheDocument();
    expect(screen.getByText('Commit Y')).toBeInTheDocument();

    toggleFocusMode();

    expect(screen.getByText('remote/master')).toBeInTheDocument();
    expect(screen.getByText('remote/stable')).toBeInTheDocument();
    expect(screen.getByText('Commit A')).toBeInTheDocument();
    expect(screen.getByText('Commit B')).toBeInTheDocument();
    expect(screen.getByText('Commit B2')).toBeInTheDocument();
    expect(screen.getByText('Commit C')).toBeInTheDocument();
    expect(screen.getByText('Commit C2')).toBeInTheDocument();
    expect(screen.getByText('Commit D')).toBeInTheDocument();
    expect(screen.getByText('Commit E')).toBeInTheDocument();
    expect(screen.getByText('Commit F')).toBeInTheDocument();

    expect(screen.queryByText('Commit X')).not.toBeInTheDocument();
    expect(screen.queryByText('Commit Y')).not.toBeInTheDocument();
  });

  it('when on a public commit, hide stacks based on the same public commit', () => {
    act(() => {
      simulateCommits({
        value: [
          COMMIT('c', 'Commit C', '1'),
          COMMIT('b', 'Commit B', '1'),
          COMMIT('a', 'Commit A', '1'),
          COMMIT('2', 'another public branch', '0', {
            phase: 'public',
            remoteBookmarks: ['remote/master'],
          }),
          COMMIT('1', 'some public base', '0', {
            phase: 'public',
            isDot: true,
            remoteBookmarks: ['remote/stable'],
          }),
        ],
      });
    });
    expect(screen.getByText('remote/master')).toBeInTheDocument();
    expect(screen.getByText('remote/stable')).toBeInTheDocument();
    expect(screen.getByText('Commit A')).toBeInTheDocument();
    expect(screen.getByText('Commit B')).toBeInTheDocument();
    expect(screen.getByText('Commit C')).toBeInTheDocument();

    toggleFocusMode();

    expect(screen.getByText('remote/master')).toBeInTheDocument();
    expect(screen.getByText('remote/stable')).toBeInTheDocument();
    expect(screen.queryByText('Commit A')).not.toBeInTheDocument();
    expect(screen.queryByText('Commit B')).not.toBeInTheDocument();
    expect(screen.queryByText('Commit C')).not.toBeInTheDocument();
  });

  it('lets you drag and drop rebase commits outside the focus stack', () => {
    jest.useFakeTimers();

    toggleFocusMode();

    dragAndDropCommits('f', '2');

    expect(screen.getAllByText('Commit F')).toHaveLength(2);
    expect(screen.getByText('Run Rebase')).toBeInTheDocument();
    jest.useRealTimers();
  });
});
