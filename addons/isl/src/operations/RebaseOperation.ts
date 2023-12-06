/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Dag, WithPreviewType} from '../previews';
import type {CommitInfo, ExactRevset, Hash, SucceedableRevset} from '../types';

import {latestSuccessor} from '../SuccessionTracker';
import {t} from '../i18n';
import {CommitPreview} from '../previews';
import {Operation} from './Operation';
import deepEqual from 'fast-deep-equal';

export class RebaseOperation extends Operation {
  constructor(
    private source: ExactRevset | SucceedableRevset,
    private destination: ExactRevset | SucceedableRevset,
  ) {
    super('RebaseOperation');
  }

  static opName = 'Rebase';

  equals(other?: Operation | null): boolean {
    return (
      other instanceof RebaseOperation &&
      deepEqual([this.source, this.destination], [other.source, other.destination])
    );
  }

  getArgs() {
    return ['rebase', '-s', this.source, '-d', this.destination];
  }

  getInitialInlineProgress(): Array<[string, string]> {
    // TODO: successions
    return [[this.source.revset, t('rebasing...')]];
  }

  previewDag(dag: Dag): Dag {
    const srcHash = dag.resolve(latestSuccessor(dag, this.source))?.hash;
    const destHash = dag.resolve(latestSuccessor(dag, this.destination))?.hash;
    if (srcHash == null || destHash == null) {
      return dag;
    }
    const src = dag.descendants(srcHash);
    const srcHashes = src.toHashes().toArray();
    const prefix = `${REBASE_PREVIEW}:${destHash}:`;
    const rewriteHash = (h: Hash) => (src.contains(h) ? prefix + h : h);
    const date = new Date();
    const newCommits = srcHashes.flatMap(h => {
      const info = dag.get(h);
      if (info == null) {
        return [];
      }
      const isRoot = info.hash === srcHash;
      const newInfo: CommitInfo & WithPreviewType = {
        ...info,
        parents: isRoot ? [destHash] : info.parents,
        date,
        seqNumber: undefined,
        previewType: isRoot ? CommitPreview.REBASE_ROOT : CommitPreview.REBASE_DESCENDANT,
      };
      return [newInfo];
    });
    // Rewrite REBASE_OLD commits to use fake hash so they won't conflict with
    // the rebased commits. Insert new commits with the original hash so they
    // can be interacted.
    return dag
      .replaceWith(src, (h, c) => {
        return (
          c && {
            ...c,
            hash: rewriteHash(h),
            parents: c.parents.map(rewriteHash),
            previewType: CommitPreview.REBASE_OLD,
          }
        );
      })
      .add(newCommits);
  }

  optimisticDag(dag: Dag): Dag {
    const src = dag.resolve(latestSuccessor(dag, this.source))?.hash;
    const dest = dag.resolve(latestSuccessor(dag, this.destination))?.hash;
    // src already on dest?
    if (src && dest && dag.parentHashes(src).includes(dest)) {
      // The stack might be partially rebased while the rebase is onging.
      // For example, graph
      //   a--b--c--d--e  z
      // Rebasing b (and descendants) to z, we might observe:
      //   a--bOld--cOld--d--e  z--bNew--cNew

      // bOld
      const srcOrig = dag.resolve(latestSuccessor(dag.remove(src), this.source))?.hash;
      // bOld + cOld + d + e
      const toRebase = dag.descendants(srcOrig);
      // bNew + cNew + d + e
      const successors = dag.followSuccessors(toRebase);
      // d + e
      const remainingToRebase = toRebase.intersect(successors);
      // cNew (simplified, does not handle merges)
      const newDest = dag.heads(dag.descendants(src).intersect(successors)).toHashes().first();

      // To test this in a real repo, try adding these configs to slow down rebases:
      // --config "hooks.pretxncommit.slow=sleep 3"
      // --config "rebase.experimental.inmemory=false"
      // and goto the src stack top before rebasing.
      return dag.rebase(remainingToRebase, newDest);
    }
    return dag.rebase(dag.descendants(src), dest);
  }
}

const REBASE_PREVIEW = 'OPTIMISTIC_REBASE_PREVIEW';
