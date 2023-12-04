/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hash} from '../types';
import type {HashWithParents} from './base_dag';

import {BaseDag} from './base_dag';

/**
 * Dag that tracks predecessor -> successor relationships.
 *
 * Precessors are "ancestors". Successors are "descendants".
 *
 * This graph can contain hashes that were deleted in the main graph.
 * For example, amend A1 to A2 to A3 to A4. This graph will keep all
 * 4 hashes so it can answer questions like "I had A1, what is it now?"
 * (A4) when the main graph only contains A4.
 *
 * In the above example, the user can also hide A4 and review A3
 * (ex. undo, or hide when A3 has visible descendants). In this case,
 * "what A1 becomes?" should be A3. So the followSuccessor()
 * implementation should consider the mainDag, like:
 *
 *     // Consider what is present in the mainDag.
 *     mutationDag.heads(mutationDag.range(start, mainDag))
 *
 * not:
 *
 *     // BAD: Only consider mutationDag, might return hashes
 *     // missing in the mainDag.
 *     mutationDag.heads(mutationDag.descendants(start))
 *
 * Note: "dag" is a lossy view for actual mutation data. For example,
 * it does not distinguish between:
 * - amending A to A1, amending A to A2 (considered "divergence")
 * - splitting A into A1 and A2 (not considered as a "divergence")
 * Be careful when it is necessary to distinguish between them.
 */
export class MutationDag extends BaseDag<HashWithParents> {
  constructor(baseDag?: BaseDag<HashWithParents>) {
    const record = baseDag?.inner;
    super(record);
  }

  /**
   * Insert old->new mappings to the mutation dag.
   *
   * Note about `oldNewPairs`:
   * - a same 'new' can have multiple 'old's (ex. fold)
   * - a same 'old' can have multiple 'new's (ex. split)
   * So the `oldNewPairs` should not be `Map`s with unique keys.
   */
  addMutations(oldNewPairs: Iterable<[Hash, Hash]>): MutationDag {
    const infoMap: Map<Hash, HashWithParents> = new Map();
    const insert = (hash: Hash, parents: Hash[]) => {
      // Insert `hash` to the infoMap on demand.
      let info = infoMap.get(hash);
      if (info == null) {
        info = {hash, parents: this.get(hash)?.parents ?? []};
        infoMap.set(hash, info);
      }
      // Append parents.
      if (parents.length > 0) {
        info.parents = Array.from(new Set(info.parents.concat(parents)));
      }
    };
    for (const [oldHash, newHash] of oldNewPairs) {
      insert(newHash, [oldHash]);
      insert(oldHash, []);
    }
    const baseDag = this.add(infoMap.values());
    return new MutationDag(baseDag);
  }
}
