/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/**
 * Benchmark for mutation-graph queries (`followSuccessors`, `successors`) on a large smartlog.
 * Not matched by the default jest `testMatch`; run explicitly from `isl/`:
 *
 *   yarn jest --testMatch '**\/__benchmarks__/**\/*.bench.ts' --testTimeout 600000
 *
 * The scenario models a user with many long stacks:
 * - `stacks` draft stacks of `depth` commits, each on its own public base.
 * - Every draft commit was amended `amends` times (hidden predecessors, mutation dag only).
 * - In every stack, `obsolete` commits in the middle were amended without restacking, so the
 *   old versions stay visible as obsolete commits with orphaned non-obsolete children.
 *
 * "stack scan" runs `orphanedChildrenInStack` for every stack root, like the
 * `orphanedChildrenForStack` atom family does whenever `latestSuccessorsMapAtom` changes,
 * i.e. on every succession batch.
 */

import type {CommitInfo, Hash} from '../../types';

import {orphanedChildrenInStack} from '../../orphanedCommits';
import {Dag, DagCommitInfo} from '../dag';

type ScenarioParams = {stacks: number; depth: number; amends: number; obsolete: number};

type Scenario = {dag: Dag; stackRoots: Array<Hash>; obsoleteHashes: Array<Hash>};

const BASE_INFO: CommitInfo = {
  title: '',
  hash: '',
  parents: [],
  grandparents: [],
  phase: 'draft',
  isDot: false,
  author: '',
  date: new Date(42),
  description: '',
  bookmarks: [],
  remoteBookmarks: [],
  totalFileCount: 0,
  filePathsSample: [],
  maxCommonPathPrefix: '',
};

function commit(info: Partial<CommitInfo>): DagCommitInfo {
  return DagCommitInfo.fromCommitInfo({...BASE_INFO, ...info});
}

function buildScenario({stacks, depth, amends, obsolete}: ScenarioParams): Scenario {
  const commits: Array<DagCommitInfo> = [];
  const history: Array<[Hash, Hash]> = [];
  const stackRoots: Array<Hash> = [];
  const obsoleteHashes: Array<Hash> = [];
  const mid = Math.floor(depth / 2);

  for (let s = 0; s < stacks; s++) {
    const base = `public-${s}`;
    commits.push(commit({hash: base, phase: 'public', parents: s > 0 ? [`public-${s - 1}`] : []}));

    let parent = base;
    for (let d = 0; d < depth; d++) {
      const hash = `s${s}-c${d}`;
      for (let a = 0; a < amends; a++) {
        history.push([`${hash}-v${a}`, a + 1 < amends ? `${hash}-v${a + 1}` : hash]);
      }
      const isObsolete = d >= mid && d < mid + obsolete;
      commits.push(
        commit({
          hash,
          parents: [parent],
          successorInfo: isObsolete ? {hash: `${hash}-new`, type: 'amend'} : undefined,
        }),
      );
      if (isObsolete) {
        obsoleteHashes.push(hash);
      }
      parent = hash;
    }
    stackRoots.push(`s${s}-c0`);

    let newParent = `s${s}-c${mid - 1}`;
    for (let d = mid; d < mid + obsolete && d < depth; d++) {
      const hash = `s${s}-c${d}-new`;
      commits.push(commit({hash, parents: [newParent], closestPredecessors: [`s${s}-c${d}`]}));
      newParent = hash;
    }
  }

  const dag = new Dag().add(commits).addMutations(history);
  return {dag, stackRoots, obsoleteHashes};
}

function stackScan(dag: Dag, roots: Array<Hash>): number {
  return roots.filter(root => orphanedChildrenInStack(dag, root) != null).length;
}

function measure(label: string, iterations: number, f: () => unknown): string {
  f(); // Warm up JIT and caches.
  const start = performance.now();
  for (let i = 0; i < iterations; i++) {
    f();
  }
  const perIteration = (performance.now() - start) / iterations;
  return `  ${label.padEnd(40)} ${perIteration.toFixed(3).padStart(10)} ms`;
}

const SIZES: Array<ScenarioParams> = [
  {stacks: 10, depth: 10, amends: 3, obsolete: 2},
  {stacks: 40, depth: 25, amends: 5, obsolete: 5},
  {stacks: 80, depth: 40, amends: 8, obsolete: 8},
];

describe('mutation dag queries', () => {
  it.each(SIZES)('stacks=$stacks depth=$depth amends=$amends obsolete=$obsolete', params => {
    const {dag, stackRoots, obsoleteHashes} = buildScenario(params);
    const sample = obsoleteHashes[Math.floor(obsoleteHashes.length / 2)];
    // A succession batch produces a new Dag (new mutationDag), invalidating per-dag caches.
    let batch = 0;
    const lines = [
      `${JSON.stringify(params)} commits=${dag.commitDag.infoMap.size} ` +
        `mutationNodes=${dag.mutationDag.infoMap.size}`,
      measure('followSuccessors(one obsolete hash)', 50, () => dag.followSuccessors(sample)),
      measure('successors(one obsolete hash)', 50, () => dag.successors(sample)),
      measure('stack scan', 3, () => stackScan(dag, stackRoots)),
      measure('succession batch + stack scan', 3, () => {
        batch++;
        const next = dag.addMutations([[`batch-${batch}-old`, `s0-c${params.depth - 1}`]]);
        return stackScan(next, stackRoots);
      }),
    ];
    process.stdout.write(lines.join('\n') + '\n');
    expect(stackScan(dag, stackRoots)).toBe(params.stacks);
  });
});
