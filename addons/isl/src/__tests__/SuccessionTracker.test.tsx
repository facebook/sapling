/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {SuccessionTracker} from '../SuccessionTracker';
import {COMMIT} from '../testUtils';

describe('SuccessionTracker', () => {
  it('finds successions', () => {
    const onSuccession = jest.fn();
    const tracker = new SuccessionTracker();
    const dispose = tracker.onSuccessions(onSuccession);

    tracker.findNewSuccessionsFromCommits([
      COMMIT('111', 'Public commit', '0', {}),
      COMMIT('aaa', 'Commit A', '1', {}),
      COMMIT('bbb', 'Commit B', '1', {}),
    ]);
    expect(onSuccession).not.toHaveBeenCalled();

    tracker.findNewSuccessionsFromCommits([
      COMMIT('111', 'Public commit', '0', {}),
      COMMIT('aaa2', 'Commit A - updated', '1', {closestPredecessors: ['aaa']}),
      COMMIT('bbb', 'Commit B', '1', {}),
    ]);
    expect(onSuccession).toHaveBeenCalledTimes(1);
    expect(onSuccession).toHaveBeenCalledWith([['aaa', 'aaa2']]);

    dispose();
  });

  it('only reports successions once', () => {
    const onSuccession = jest.fn();
    const tracker = new SuccessionTracker();
    const dispose = tracker.onSuccessions(onSuccession);

    tracker.findNewSuccessionsFromCommits([
      COMMIT('111', 'Public commit', '0', {}),
      COMMIT('aaa', 'Commit A', '1', {}),
      COMMIT('bbb', 'Commit B', '1', {}),
    ]);

    tracker.findNewSuccessionsFromCommits([
      COMMIT('111', 'Public commit', '0', {}),
      COMMIT('aaa2', 'Commit A - updated', '1', {closestPredecessors: ['aaa']}),
      COMMIT('bbb', 'Commit B', '1', {}),
    ]);
    tracker.findNewSuccessionsFromCommits([
      COMMIT('111', 'Public commit', '0', {}),
      COMMIT('aaa2', 'Commit A - updated', '1', {closestPredecessors: ['aaa']}),
      COMMIT('bbb', 'Commit B', '1', {}),
    ]);
    expect(onSuccession).toHaveBeenCalledTimes(1);
    expect(onSuccession).toHaveBeenCalledWith([['aaa', 'aaa2']]);

    dispose();
  });

  it('skips new commits even if they have predecessors', () => {
    const onSuccession = jest.fn();
    const tracker = new SuccessionTracker();
    const dispose = tracker.onSuccessions(onSuccession);

    tracker.findNewSuccessionsFromCommits([
      COMMIT('111', 'Public commit', '0', {}),
      COMMIT('aaa', 'Commit A', '1', {closestPredecessors: ['a0']}),
      COMMIT('bbb', 'Commit B', '1', {closestPredecessors: ['b0']}),
    ]);
    expect(onSuccession).not.toHaveBeenCalled();
    dispose();
  });

  it('handles multiple predecessors (e.g. from fold)', () => {
    const onSuccession = jest.fn();
    const tracker = new SuccessionTracker();
    const dispose = tracker.onSuccessions(onSuccession);

    tracker.findNewSuccessionsFromCommits([
      COMMIT('111', 'Public commit', '0', {}),
      COMMIT('aaa', 'Commit A', '1', {}),
      COMMIT('bbb', 'Commit B', 'aaa', {}),
      COMMIT('ccc', 'Commit C', 'bbb', {}),
    ]);
    expect(onSuccession).not.toHaveBeenCalled();

    tracker.findNewSuccessionsFromCommits([
      COMMIT('111', 'Public commit', '0', {}),
      COMMIT('aaa', 'Commit A', '1', {}),
      COMMIT('bc', 'Fold B & C', '1', {closestPredecessors: ['bbb', 'ccc']}),
    ]);
    expect(onSuccession).toHaveBeenCalledTimes(1);
    expect(onSuccession).toHaveBeenCalledWith([
      ['bbb', 'bc'],
      ['ccc', 'bc'],
    ]);

    dispose();
  });
});
