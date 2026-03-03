/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {GitHubDiffSummary} from 'isl-server/src/github/githubCodeReviewProvider';
import type {DiffId, DiffSummary} from '../types';

import {createStore} from 'jotai';
import {PullRequestState} from 'isl-server/src/github/generated/graphql';
import {allDiffSummaries} from '../codeReview/CodeReviewInfo';
import {prStacksAtom} from '../codeReview/PRStacksAtom';
import type {StackEntry} from 'isl-server/src/github/parseStackInfo';

/** Helper to create a mock GitHub PR diff summary. */
function mockPR(
  number: number,
  opts: {
    state?: PullRequestState | 'DRAFT';
    branchName?: string;
    baseRefName?: string;
    stackInfo?: StackEntry[];
    author?: string;
  } = {},
): GitHubDiffSummary {
  return {
    type: 'github',
    number: String(number),
    nodeId: `node-${number}`,
    title: `PR #${number}`,
    commitMessage: `commit for PR #${number}`,
    state: opts.state ?? PullRequestState.Open,
    url: `https://github.com/test/repo/pull/${number}`,
    anyUnresolvedComments: false,
    commentCount: 0,
    base: `base-${number}`,
    head: `head-${number}`,
    branchName: opts.branchName ?? `pr${number}`,
    baseRefName: opts.baseRefName ?? 'main',
    stackInfo: opts.stackInfo,
    author: opts.author ?? 'testuser',
  };
}

function setupStore(prs: GitHubDiffSummary[]) {
  const store = createStore();
  const map = new Map<DiffId, DiffSummary>();
  for (const pr of prs) {
    map.set(String(pr.number), pr as DiffSummary);
  }
  store.set(allDiffSummaries, {value: map});
  return store;
}

describe('prStacksAtom branch-chain detection', () => {
  it('groups PRs with Sapling footers into a stack (baseline)', () => {
    const store = setupStore([
      mockPR(100, {
        stackInfo: [
          {isCurrent: false, prNumber: 102},
          {isCurrent: false, prNumber: 101},
          {isCurrent: true, prNumber: 100},
        ],
      }),
      mockPR(101, {
        stackInfo: [
          {isCurrent: false, prNumber: 102},
          {isCurrent: true, prNumber: 101},
          {isCurrent: false, prNumber: 100},
        ],
      }),
      mockPR(102, {
        stackInfo: [
          {isCurrent: true, prNumber: 102},
          {isCurrent: false, prNumber: 101},
          {isCurrent: false, prNumber: 100},
        ],
      }),
    ]);

    const stacks = store.get(prStacksAtom);
    expect(stacks).toHaveLength(1);
    expect(stacks[0].isStack).toBe(true);
    expect(stacks[0].prs.map(p => Number(p.type === 'github' ? p.number : 0))).toEqual([
      102, 101, 100,
    ]);
  });

  it('merges branch-chained PRs on top of a footer-based stack', () => {
    // Simulates: 100 → 101 → 102 (footer stack), then 103 → 104 → 105 chain on top
    const footerStack: StackEntry[] = [
      {isCurrent: false, prNumber: 102},
      {isCurrent: false, prNumber: 101},
      {isCurrent: false, prNumber: 100},
    ];
    const store = setupStore([
      mockPR(100, {stackInfo: footerStack.map((e, i) => ({...e, isCurrent: i === 2}))}),
      mockPR(101, {stackInfo: footerStack.map((e, i) => ({...e, isCurrent: i === 1}))}),
      mockPR(102, {stackInfo: footerStack.map((e, i) => ({...e, isCurrent: i === 0}))}),
      // PRs stacked via branch targeting (no Sapling footers)
      mockPR(103, {baseRefName: 'pr102', author: 'otheruser'}),
      mockPR(104, {baseRefName: 'pr103', author: 'otheruser'}),
      mockPR(105, {baseRefName: 'pr104', author: 'otheruser'}),
    ]);

    const stacks = store.get(prStacksAtom);
    // All 6 PRs should be in a single stack
    expect(stacks).toHaveLength(1);
    expect(stacks[0].isStack).toBe(true);
    const prNumbers = stacks[0].prs.map(p => Number(p.type === 'github' ? p.number : 0));
    // Order: newest at top → 105, 104, 103, 102, 101, 100
    expect(prNumbers).toEqual([105, 104, 103, 102, 101, 100]);
    expect(stacks[0].topPrNumber).toBe(105);
  });

  it('forms a new stack from orphan branch-chained singles', () => {
    // PRs that chain together via baseRefName but have no Sapling footers
    const store = setupStore([
      mockPR(200, {baseRefName: 'main'}),
      mockPR(201, {baseRefName: 'pr200'}),
      mockPR(202, {baseRefName: 'pr201'}),
    ]);

    const stacks = store.get(prStacksAtom);
    expect(stacks).toHaveLength(1);
    expect(stacks[0].isStack).toBe(true);
    const prNumbers = stacks[0].prs.map(p => Number(p.type === 'github' ? p.number : 0));
    expect(prNumbers).toEqual([202, 201, 200]);
  });

  it('does not merge a PR whose baseRefName does not match any known branch', () => {
    const store = setupStore([
      mockPR(300, {baseRefName: 'main'}),
      mockPR(301, {baseRefName: 'some-unrelated-branch'}),
    ]);

    const stacks = store.get(prStacksAtom);
    expect(stacks).toHaveLength(2);
    expect(stacks.every(s => !s.isStack)).toBe(true);
  });

  it('does not create duplicate entries when footer and branch-chain overlap', () => {
    // PR 101 is in the footer stack AND its baseRefName points to pr100
    // The branch-chain detection should not add 101 twice
    const footerStack: StackEntry[] = [
      {isCurrent: false, prNumber: 101},
      {isCurrent: false, prNumber: 100},
    ];
    const store = setupStore([
      mockPR(100, {
        stackInfo: footerStack.map((e, i) => ({...e, isCurrent: i === 1})),
      }),
      mockPR(101, {
        baseRefName: 'pr100',
        stackInfo: footerStack.map((e, i) => ({...e, isCurrent: i === 0})),
      }),
    ]);

    const stacks = store.get(prStacksAtom);
    expect(stacks).toHaveLength(1);
    expect(stacks[0].prs).toHaveLength(2);
  });

  it('handles mixed: footer stack + branch chain + unrelated singles', () => {
    const footerStack: StackEntry[] = [
      {isCurrent: false, prNumber: 11},
      {isCurrent: false, prNumber: 10},
    ];
    const store = setupStore([
      mockPR(10, {stackInfo: footerStack.map((e, i) => ({...e, isCurrent: i === 1}))}),
      mockPR(11, {stackInfo: footerStack.map((e, i) => ({...e, isCurrent: i === 0}))}),
      mockPR(12, {baseRefName: 'pr11'}), // chains onto footer stack
      mockPR(50, {baseRefName: 'main'}), // unrelated single
    ]);

    const stacks = store.get(prStacksAtom);
    // Should have 2 stacks: the extended footer stack (10,11,12) and the single (50)
    expect(stacks).toHaveLength(2);
    const multiStack = stacks.find(s => s.isStack);
    const single = stacks.find(s => !s.isStack);
    expect(multiStack).toBeDefined();
    expect(single).toBeDefined();
    expect(multiStack!.prs.map(p => Number(p.type === 'github' ? p.number : 0))).toEqual([
      12, 11, 10,
    ]);
    expect(Number(single!.prs[0].type === 'github' ? single!.prs[0].number : 0)).toBe(50);
  });

  it('does not duplicate PRs when chain descendants appear before footer stack in map', () => {
    // Reproduces the real bug: GitHub returns recently-updated PRs first,
    // so 4605,4604,4603 are before the footer stack in the map.
    // The algorithm must not let singles grab each other before the footer stack
    // claims its descendants.
    const footerEntries: StackEntry[] = [
      {isCurrent: false, prNumber: 4565},
      {isCurrent: false, prNumber: 4564},
      {isCurrent: false, prNumber: 4563},
      {isCurrent: false, prNumber: 4562},
      {isCurrent: false, prNumber: 4561},
      {isCurrent: false, prNumber: 4560},
      {isCurrent: false, prNumber: 4559},
    ];
    // Insert in the order GitHub returns them (newest first)
    const store = setupStore([
      mockPR(4605, {baseRefName: 'pr4604', author: 'sontiO'}),
      mockPR(4604, {baseRefName: 'pr4603', author: 'sontiO'}),
      mockPR(4603, {baseRefName: 'pr4565', author: 'sontiO'}),
      mockPR(4565, {
        stackInfo: footerEntries.map((e, i) => ({...e, isCurrent: i === 0})),
        author: 'Lennix',
      }),
      mockPR(4564, {
        stackInfo: footerEntries.map((e, i) => ({...e, isCurrent: i === 1})),
        author: 'Lennix',
      }),
      mockPR(4563, {
        stackInfo: footerEntries.map((e, i) => ({...e, isCurrent: i === 2})),
        author: 'Lennix',
      }),
      mockPR(4562, {
        stackInfo: footerEntries.map((e, i) => ({...e, isCurrent: i === 3})),
        author: 'Lennix',
      }),
      mockPR(4561, {
        stackInfo: footerEntries.map((e, i) => ({...e, isCurrent: i === 4})),
        author: 'Lennix',
      }),
      mockPR(4560, {
        stackInfo: footerEntries.map((e, i) => ({...e, isCurrent: i === 5})),
        author: 'Lennix',
      }),
      mockPR(4559, {
        stackInfo: footerEntries.map((e, i) => ({...e, isCurrent: i === 6})),
        author: 'Lennix',
      }),
    ]);

    const stacks = store.get(prStacksAtom);
    // Must be exactly 1 stack with all 10 PRs, no duplicates
    expect(stacks).toHaveLength(1);
    expect(stacks[0].isStack).toBe(true);
    const prNumbers = stacks[0].prs.map(p => Number(p.type === 'github' ? p.number : 0));
    expect(prNumbers).toEqual([4605, 4604, 4603, 4565, 4564, 4563, 4562, 4561, 4560, 4559]);

    // No PR should appear more than once across all stacks
    const allPrNumbers = stacks.flatMap(s =>
      s.prs.map(p => (p.type === 'github' ? p.number : '')),
    );
    expect(new Set(allPrNumbers).size).toBe(allPrNumbers.length);
  });

  it('correctly computes metadata after branch-chain merge', () => {
    const footerStack: StackEntry[] = [
      {isCurrent: false, prNumber: 2},
      {isCurrent: false, prNumber: 1},
    ];
    const store = setupStore([
      mockPR(1, {
        state: PullRequestState.Merged,
        stackInfo: footerStack.map((e, i) => ({...e, isCurrent: i === 1})),
        author: 'alice',
      }),
      mockPR(2, {
        stackInfo: footerStack.map((e, i) => ({...e, isCurrent: i === 0})),
        author: 'alice',
      }),
      mockPR(3, {baseRefName: 'pr2', author: 'bob'}),
    ]);

    const stacks = store.get(prStacksAtom);
    expect(stacks).toHaveLength(1);
    const stack = stacks[0];
    expect(stack.topPrNumber).toBe(3);
    // Main author comes from the top PR (newest)
    expect(stack.mainAuthor).toBe('bob');
    expect(stack.mergedCount).toBe(1);
    expect(stack.isMerged).toBe(false); // not all merged
  });

  it('collects unique authors from all PRs in a multi-author stack', () => {
    const footerStack: StackEntry[] = [
      {isCurrent: false, prNumber: 2},
      {isCurrent: false, prNumber: 1},
    ];
    const store = setupStore([
      mockPR(1, {
        stackInfo: footerStack.map((e, i) => ({...e, isCurrent: i === 1})),
        author: 'alice',
      }),
      mockPR(2, {
        stackInfo: footerStack.map((e, i) => ({...e, isCurrent: i === 0})),
        author: 'alice',
      }),
      mockPR(3, {baseRefName: 'pr2', author: 'bob'}),
      mockPR(4, {baseRefName: 'pr3', author: 'bob'}),
    ]);

    const stacks = store.get(prStacksAtom);
    expect(stacks).toHaveLength(1);
    const stack = stacks[0];
    // Should have exactly 2 unique authors: bob (top), alice
    expect(stack.authors).toHaveLength(2);
    expect(stack.authors.map(a => a.login)).toEqual(['bob', 'alice']);
  });
});
