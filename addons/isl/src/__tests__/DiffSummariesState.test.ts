/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DiffId, DiffSummary} from '../types';

import {act} from '@testing-library/react';
import {PullRequestState} from '../../../isl-server/src/github/generated/graphql';
import {allDiffSummaries, branchingDiffInfos, diffSummary} from '../codeReview/CodeReviewInfo';
import {readAtom, writeAtom} from '../jotaiUtils';
import {simulateMessageFromServer} from '../testUtils';

function PR(number: DiffId, branchName?: string): DiffSummary {
  return {
    number,
    branchName,
    state: PullRequestState.Open,
    title: `PR ${number}`,
    type: 'github',
    url: `https://github.com/myusername/testrepo/pull/${number}`,
    anyUnresolvedComments: false,
    commentCount: 0,
    commitMessage: `PR ${number}`,
    base: 'a',
    head: 'b',
  };
}

function receiveSummaries(...numbers: Array<DiffId>) {
  act(() =>
    simulateMessageFromServer({
      type: 'fetchedDiffSummaries',
      summaries: {value: new Map(numbers.map(number => [number, PR(number)]))},
    }),
  );
}

function receiveError(message: string) {
  act(() =>
    simulateMessageFromServer({
      type: 'fetchedDiffSummaries',
      summaries: {error: new Error(message)},
    }),
  );
}

/** A result from a provider that reports its whole set of failing diffs with every result. */
function receiveWithFailures(
  delivered: Array<DiffId>,
  failures: Array<[DiffId, string]>,
  asError?: string,
) {
  // Grouped by message, as a provider groups the diffs that share an error.
  const byMessage = new Map<string, Array<DiffId>>();
  for (const [diffId, message] of failures) {
    byMessage.set(message, [...(byMessage.get(message) ?? []), diffId]);
  }
  const grouped = [...byMessage].map(([message, diffIds]) => ({
    error: new Error(message),
    diffIds,
  }));
  act(() =>
    simulateMessageFromServer({
      type: 'fetchedDiffSummaries',
      summaries:
        asError == null
          ? {value: new Map(delivered.map(number => [number, PR(number)])), failures: grouped}
          : {error: new Error(asError), failures: grouped},
    }),
  );
}

describe('diff summaries state', () => {
  beforeEach(() => {
    act(() => writeAtom(allDiffSummaries, {value: null}));
  });

  describe('from a provider that does not report failures per diff', () => {
    it('replaces every summary with a fetch error', () => {
      receiveSummaries('10');
      receiveError('interngraph fatal');

      expect(readAtom(allDiffSummaries).error?.message).toBe('interngraph fatal');
      expect(readAtom(diffSummary('10')).error?.message).toBe('interngraph fatal');
      expect(readAtom(diffSummary('11')).error?.message).toBe('interngraph fatal');
    });

    it('replaces the error with the next summaries', () => {
      receiveSummaries('10');
      receiveError('interngraph fatal');
      receiveSummaries('11');

      const state = readAtom(allDiffSummaries);
      expect(state.error).toBeUndefined();
      expect([...(state.value?.keys() ?? [])]).toEqual(['11']);
    });

    it('merges summaries that arrive in parts', () => {
      receiveSummaries('10');
      receiveSummaries('11');

      expect([...(readAtom(allDiffSummaries).value?.keys() ?? [])]).toEqual(['10', '11']);
    });
  });

  describe('from a provider reporting failures per diff', () => {
    it('shows each failing diff its own error and keeps summaries on screen', () => {
      receiveSummaries('10');
      receiveWithFailures(
        [],
        [
          ['11', 'timed out'],
          ['12', 'VPN down'],
        ],
        'VPN down',
      );

      expect(readAtom(diffSummary('10')).value?.title).toBe('PR 10');
      expect(readAtom(diffSummary('11')).error?.message).toBe('timed out');
      expect(readAtom(diffSummary('12')).error?.message).toBe('VPN down');
      expect(readAtom(diffSummary('13'))).toEqual({value: undefined});
    });

    it('keeps every distinct failure in the banner', () => {
      receiveWithFailures(
        [],
        [
          ['11', 'timed out'],
          ['12', 'VPN down'],
          ['13', 'VPN down'],
        ],
        'x',
      );

      expect(readAtom(allDiffSummaries).error?.message).toBe(
        'Failed to fetch diff summaries: VPN down; timed out',
      );
    });

    it('keeps the banner error while the failures say the same thing', () => {
      // A new error is a new banner to the tracker, which would log the same failure again.
      receiveWithFailures([], [['11', 'timed out']], 'timed out');
      const single = readAtom(allDiffSummaries).error;
      receiveWithFailures(['10'], [['11', 'timed out']]);
      expect(readAtom(allDiffSummaries).error).toBe(single);

      receiveWithFailures(
        [],
        [
          ['11', 'timed out'],
          ['12', 'VPN down'],
        ],
        'VPN down',
      );
      const combined = readAtom(allDiffSummaries).error;
      expect(combined).not.toBe(single);
      receiveWithFailures(
        ['13'],
        [
          ['12', 'VPN down'],
          ['11', 'timed out'],
        ],
      );
      expect(readAtom(allDiffSummaries).error).toBe(combined);
    });

    it('shows a single failure in the banner as is', () => {
      receiveWithFailures([], [['11', 'interngraph fatal']], 'interngraph fatal');

      expect(readAtom(allDiffSummaries).error?.message).toBe('interngraph fatal');
    });

    it('keeps failures the provider still reports when summaries for other diffs arrive', () => {
      receiveWithFailures([], [['11', 'interngraph fatal']], 'interngraph fatal');

      receiveWithFailures(['10'], [['11', 'interngraph fatal']]);

      expect(readAtom(diffSummary('11')).error?.message).toBe('interngraph fatal');
    });

    it('clears once the provider reports no failures', () => {
      receiveWithFailures([], [['11', 'interngraph fatal']], 'interngraph fatal');

      receiveWithFailures(['12'], []);

      expect(readAtom(allDiffSummaries).error).toBeUndefined();
      expect(readAtom(diffSummary('11'))).toEqual({value: undefined});
    });
  });

  // The provider stops or starts reporting failures per diff when its GK flips mid-session.
  describe('when the result shape changes', () => {
    it('replaces per-diff failures with summaries without them', () => {
      receiveWithFailures(['10'], [['11', 'interngraph fatal']], 'interngraph fatal');
      receiveSummaries('12');

      expect(readAtom(allDiffSummaries).error).toBeUndefined();
      expect(readAtom(diffSummary('11'))).toEqual({value: undefined});
      expect([...(readAtom(allDiffSummaries).value?.keys() ?? [])]).toEqual(['12']);
    });

    it('replaces per-diff failures with an error without them', () => {
      receiveWithFailures(['10'], [['11', 'timed out']]);
      receiveError('interngraph fatal');

      expect(readAtom(diffSummary('10')).error?.message).toBe('interngraph fatal');
      expect(readAtom(diffSummary('11')).error?.message).toBe('interngraph fatal');
    });

    it('replaces an error without per-diff failures with one that has them', () => {
      receiveError('interngraph fatal');
      receiveWithFailures([], [['11', 'timed out']], 'timed out');

      expect(readAtom(allDiffSummaries).error?.message).toBe('timed out');
      expect(readAtom(diffSummary('11')).error?.message).toBe('timed out');
      expect(readAtom(diffSummary('12'))).toEqual({value: undefined});
    });
  });

  describe('branching PRs', () => {
    it("show the branch's summary", () => {
      act(() =>
        simulateMessageFromServer({
          type: 'fetchedDiffSummaries',
          summaries: {value: new Map([['10', PR('10', 'feature')]])},
        }),
      );

      expect(readAtom(branchingDiffInfos('feature')).value?.title).toBe('PR 10');
      expect(readAtom(branchingDiffInfos('other'))).toEqual({value: undefined});
    });

    it('show a fetch error on every branch', () => {
      act(() =>
        simulateMessageFromServer({
          type: 'fetchedDiffSummaries',
          summaries: {value: new Map([['10', PR('10', 'feature')]])},
        }),
      );
      receiveError('interngraph fatal');

      expect(readAtom(branchingDiffInfos('feature')).error?.message).toBe('interngraph fatal');
      expect(readAtom(branchingDiffInfos('other')).error?.message).toBe('interngraph fatal');
    });
  });
});
