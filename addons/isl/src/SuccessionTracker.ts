/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Dag} from './dag/dag';
import type {CommitInfo, ExactRevset, SmartlogCommits, SucceedableRevset} from './types';

import {MutationDag} from './dag/mutation_dag';
import {exactRevset, succeedableRevset} from './types';
import {atom, DefaultValue} from 'recoil';

type Successions = Array<[oldHash: string, newHash: string]>;
type SuccessionCallback = (successions: Successions) => unknown;

/**
 * When a commit is amended or rebased or otherwise modified, the old commit
 * is marked obsolete and "succeeded" by a new commit.
 * Some state in the UI is keyed by hash, so a succession event can cause the UI
 * to show stale data. For example, if you select a commit and amend a commit earlier in the stack,
 * your selection will now disappear.
 *
 * To avoid this, we keep track of commits being succeeded, and any recoil state keyed on hashes
 * can listen to this event and update itself with the new oldHash -> newHash.
 *
 * Succession is tracked by looking at each new batch of commits we get, each of which may have
 * a closestPredecessors field. Technically, it's probably possible that a commit is succeeded twice
 * between results from `sl log`, which would cause us to miss a succession. We'll ignore this case for now,
 * and assume it's rare.
 *
 * Note that successions could also be detected on the server by stdout or other means from sl,
 * but by doing it on the client we know that all successions are dealt with at the exact moment the
 * UI state gets a new list of commits, reducing any race between succession events and new commits rendering.
 */
export class SuccessionTracker {
  private callbacks: Set<SuccessionCallback> = new Set();
  /**
   * Run a callback when a succession is detected for the first time.
   * Returns a dispose function.
   */
  public onSuccessions(cb: SuccessionCallback): () => void {
    this.callbacks.add(cb);
    return () => {
      this.callbacks.delete(cb);
    };
  }

  private seenHashes = new Set<string>();
  /**
   * Called once in the app each time a new batch of commits is fetched,
   * in order to find successions and run callbacks on them.
   */
  public findNewSuccessionsFromCommits(commits: SmartlogCommits) {
    const successions: Successions = [];
    for (const commit of commits) {
      if (commit.phase === 'public') {
        continue;
      }

      const {hash: newHash, closestPredecessors: oldHashes} = commit;

      // Commits we've seen before should have already had their successions computed, so they are skipped

      // Commits we've never seen before, who have predecessors we've never seen are just entirely new commits
      // or from our first time fetching commits. Skip computing predecessors for these.

      // Commits we've *never* seen before, who have predecessors that we *have* seen before are actually successions.
      if (oldHashes != null && !this.seenHashes.has(newHash)) {
        for (const oldHash of oldHashes) {
          if (this.seenHashes.has(oldHash)) {
            successions.push([oldHash, newHash]);
          }
        }
      }

      this.seenHashes.add(newHash);
    }

    if (successions.length > 0) {
      for (const cb of this.callbacks) {
        cb(successions);
      }
    }
  }

  /** Clear all known hashes and remove all listeners, useful for resetting between tests */
  public clear() {
    this.seenHashes.clear();
    this.callbacks.clear();
  }
}

export const successionTracker = new SuccessionTracker();

export const latestSuccessorsMap = atom<MutationDag>({
  key: 'latestSuccessorsMap',
  default: new MutationDag(),
  effects: [
    ({setSelf}) => {
      return successionTracker.onSuccessions(successions => {
        setSelf(existing => {
          const dag = existing instanceof DefaultValue ? new MutationDag() : existing;
          return dag.addMutations(successions);
        });
      });
    },
  ],
});

/**
 * Get the latest successor hash of the given hash,
 * traversing multiple successions if necessary.
 * Returns original hash if no successors were found.
 *
 * Useful for previews to ensure they're working with the latest version of a commit,
 * given that they may have been queued up while another operation ran and eventually caused succession.
 *
 * Note: if an ExactRevset is passed, don't look up the successor.
 */
export function latestSuccessor(ctx: Dag, oldRevset: SucceedableRevset | ExactRevset): string {
  let hash = oldRevset.revset;
  if (oldRevset.type === 'exact-revset') {
    return hash;
  }
  hash = ctx.followSuccessors(hash).toHashes().first() ?? hash;
  return hash;
}

/**
 * Typically we want to use succeedable revsets everywhere, to maximize support for queued commands.
 * But if you see and act on a visibly obsolete commit in the UI, we should use its exact hash,
 * so that you don't suddenly act on a seemingly unrelated commit.
 */
export function latestSuccessorUnlessExplicitlyObsolete(
  commit: CommitInfo,
): SucceedableRevset | ExactRevset {
  if (commit.successorInfo?.type != null) {
    return exactRevset(commit.hash);
  }
  return succeedableRevset(commit.hash);
}
