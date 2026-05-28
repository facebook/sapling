/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Dag} from '../previews';
import type {ExactRevset, OptimisticRevset, SucceedableRevset} from '../types';

import {Operation} from './Operation';

type RevsetSource = SucceedableRevset | ExactRevset | OptimisticRevset;

export class CommitCloudMoveCommitsOperation extends Operation {
  static opName = 'CommitCloudMoveCommits';

  constructor(
    private destinationWorkspace: string,
    private sources: ReadonlyArray<RevsetSource>,
  ) {
    super('CommitCloudMoveCommitsOperation');
  }

  getArgs() {
    // `sl cloud move` follows descendants automatically
    // (commitcloud/move.py: `dag.descendants(removenodes)`) and carries
    // attached bookmarks with them, so we only need to pass the
    // user-selected commit hashes.
    const args: Array<string | RevsetSource> = ['cloud', 'move', '-d', this.destinationWorkspace];
    for (const source of this.sources) {
      args.push('-r', source);
    }
    return args;
  }

  private hashes(): string[] {
    return this.sources.map(source =>
      source.type === 'optimistic-revset' ? source.fake : source.revset,
    );
  }

  optimisticDag(dag: Dag): Dag {
    // Mirror sl's behavior: `cloud move` walks `dag.descendants(removenodes)`
    // (commitcloud/move.py) and drops them from the current workspace.
    // Unlike `sl hide`, `sl cloud move` never auto-gotos the parent when `.`
    // is among the moved commits — it only updates cloud refs via
    // `serv.updatereferences(...)` — so we deliberately skip HideOp's
    // head-hop. The local working copy stays where it is.
    const hashes = this.hashes();
    const toRemove = dag.descendants(hashes);
    const toCleanup = dag.parents(hashes);
    return dag.remove(toRemove).cleanup(toCleanup);
  }
}
