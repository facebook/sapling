/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Tracker} from 'isl-server/src/analytics/tracker';

import {SuccessionTracker} from '../SuccessionTracker';
import {Dag, DagCommitInfo} from '../dag/dag';
import {COMMIT} from '../testUtils';

describe('SuccessionTracker', () => {
  const dag = new Dag().add(
    [
      COMMIT('111', 'Public commit', '0', {}),
      COMMIT('aaa', 'Commit A', '1', {}),
      COMMIT('bbb', 'Commit B', '1', {}),
    ].map(DagCommitInfo.fromCommitInfo),
  );
  it('finds successions', () => {
    const onSuccession = jest.fn();
    const tracker = new SuccessionTracker();
    const dispose = tracker.onSuccessions(onSuccession);

    tracker.findNewSuccessionsFromCommits(dag, [
      COMMIT('111', 'Public commit', '0', {}),
      COMMIT('aaa', 'Commit A', '1', {}),
      COMMIT('bbb', 'Commit B', '1', {}),
    ]);
    expect(onSuccession).not.toHaveBeenCalled();

    tracker.findNewSuccessionsFromCommits(dag, [
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

    tracker.findNewSuccessionsFromCommits(dag, [
      COMMIT('111', 'Public commit', '0', {}),
      COMMIT('aaa', 'Commit A', '1', {}),
      COMMIT('bbb', 'Commit B', '1', {}),
    ]);

    tracker.findNewSuccessionsFromCommits(dag, [
      COMMIT('111', 'Public commit', '0', {}),
      COMMIT('aaa2', 'Commit A - updated', '1', {closestPredecessors: ['aaa']}),
      COMMIT('bbb', 'Commit B', '1', {}),
    ]);
    tracker.findNewSuccessionsFromCommits(dag, [
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

    tracker.findNewSuccessionsFromCommits(dag, [
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

    tracker.findNewSuccessionsFromCommits(dag, [
      COMMIT('111', 'Public commit', '0', {}),
      COMMIT('aaa', 'Commit A', '1', {}),
      COMMIT('bbb', 'Commit B', 'aaa', {}),
      COMMIT('ccc', 'Commit C', 'bbb', {}),
    ]);
    expect(onSuccession).not.toHaveBeenCalled();

    tracker.findNewSuccessionsFromCommits(dag, [
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

  it('looks for "buggy" successsions that modify diff numbers', () => {
    const onSuccession = jest.fn();
    const tracker = new SuccessionTracker();
    const dispose = tracker.onSuccessions(onSuccession);
    const mockTrack = jest.fn();

    window.globalIslClientTracker = {track: mockTrack} as unknown as Tracker<Record<string, never>>;

    let dag = new Dag().add(
      [
        COMMIT('111', 'Public commit', '0', {}),
        COMMIT('aaa', 'Commit A', '1', {}),
        COMMIT('bbb', 'Commit B', '1', {}),
      ].map(DagCommitInfo.fromCommitInfo),
    );

    tracker.findNewSuccessionsFromCommits(dag, [
      COMMIT('111', 'Public commit', '0', {}),
      COMMIT('aaa', 'Commit A', '1', {diffId: 'D111', description: 'old: D111'}),
      COMMIT('bbb', 'Commit B', '1', {diffId: 'D222'}),
    ]);
    expect(onSuccession).not.toHaveBeenCalled();

    dag = new Dag().add(
      [
        COMMIT('111', 'Public commit', '0', {}),
        COMMIT('aaa', 'Commit A', '1', {diffId: 'D111', description: 'old: D111'}),
        COMMIT('bbb', 'Commit B', '1', {diffId: 'D222'}),
      ].map(DagCommitInfo.fromCommitInfo),
    );

    tracker.findNewSuccessionsFromCommits(dag, [
      COMMIT('111', 'Public commit', '0', {}),
      COMMIT('aaa2', 'Commit A - updated', '1', {
        closestPredecessors: ['aaa'],
        diffId: 'D333',
        description: 'new: D333',
      }),
      COMMIT('bbb', 'Commit B', '1', {diffId: 'D222'}),
    ]);
    expect(onSuccession).toHaveBeenCalledTimes(0);

    expect(mockTrack).toHaveBeenCalledWith('BuggySuccessionDetected', {
      extras: {
        oldHash: 'aaa',
        newHash: 'aaa2',
        old: 'Commit A\nold: D111',
        new: 'Commit A - updated\nnew: D333',
      },
    });

    dispose();

    delete (window as {globalIslClientTracker?: unknown}).globalIslClientTracker;
  });
});
