/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitStackState} from './commitStackState';

import {cached} from 'shared/LRU';
import {firstLine, nullthrows} from 'shared/utils';
import {Dag, DagCommitInfo} from '../dag/dag';
import {WDIR_NODE, YOU_ARE_HERE_VIRTUAL_COMMIT} from '../dag/virtualCommit';

/**
 * Calculate a virtual "Dag" purely from "stack".
 * The virtual "Dag" does not have to be a subset of the main dag being displayed.
 * This avoids race conditions and various issues when the "stack" does not
 * match the main dag.
 * The nodes (commit hashes) of the returned dag is the "key" of commits in
 * the "stack", not real commit hashes. This is to support various stack edit
 * operations like splitting, inserting new commits, etc.
 */
function calculateDagFromStackImpl(stack: CommitStackState): Dag {
  let dag = new Dag();

  // Figure out the "dot" from the initial exported stack.
  // "dot" is the parent of "wdir()".
  // The exported stack might not have "wdir()" if not requested.
  // Run `sl debugexportstack -r '.+wdir()' | python3 -m json.tool` to get a sense of the output.
  let dotNode: string | undefined = undefined;
  let dotKey: string | undefined = undefined;
  for (const exportedCommit of stack.originalStack) {
    if (exportedCommit.node === WDIR_NODE) {
      dotNode = exportedCommit.parents?.at(0);
      break;
    }
  }

  if (dotNode != null) {
    const maybeDotCommit = stack.stack.findLast(commit => commit.originalNodes.contains(dotNode));
    if (maybeDotCommit != null) {
      dotKey = maybeDotCommit.key;
      const wdirRev = stack.findLastRev(commit => commit.originalNodes.contains(WDIR_NODE));
      dag = dag.add([YOU_ARE_HERE_VIRTUAL_COMMIT.merge({parents: [dotKey], stackRev: wdirRev})]);
    }
  }

  // Insert commits from the stack.
  // Since we've already inserted the "wdir()" commit, skip it from the stack.
  dag = dag.add(
    stack
      .revs()
      .filter(rev => !stack.get(rev)?.originalNodes?.contains(WDIR_NODE))
      .map(rev => {
        const commit = nullthrows(stack.get(rev));
        return DagCommitInfo.fromCommitInfo({
          title: firstLine(commit.text),
          hash: commit.key,
          parents: commit.parents
            .flatMap(parentRev => {
              const parent = stack.get(parentRev);
              return parent == null ? [] : [parent.key];
            })
            .toArray(),
          phase: commit.immutableKind === 'hash' ? 'public' : 'draft',
          author: commit.author,
          date: new Date(commit.date.unix),
          isDot: commit.key === dotKey,
          stackRev: rev,
          // Other fields are omitted for now, since nobody uses them yet.
        });
      }),
  );

  return dag;
}

/**
 * Provides a `Dag` that just contains the `stack`.
 * If `dotRev` is set, add a "YouAreHere" virtual commit as a child of the rev.
 */
export const calculateDagFromStack = cached(calculateDagFromStackImpl);
