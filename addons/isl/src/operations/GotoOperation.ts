/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Dag} from '../previews';
import type {ExactRevset, OptimisticRevset, SucceedableRevset} from '../types';

import {YOU_ARE_HERE_VIRTUAL_COMMIT} from '../dag/virtualCommit';
import {t} from '../i18n';
import {CommitPreview} from '../previews';
import {latestSuccessor} from '../successionUtils';
import {GotoBaseOperation} from './GotoBaseOperation';

export class GotoOperation extends GotoBaseOperation {
  constructor(protected destination: SucceedableRevset | ExactRevset | OptimisticRevset) {
    super(destination);
  }

  getInitialInlineProgress(): [hash: string, message: string][] {
    return [[YOU_ARE_HERE_VIRTUAL_COMMIT.hash, t('moving...')]];
  }

  optimisticDag(dag: Dag): Dag {
    const headCommitHash = dag.resolve('.')?.hash;
    if (headCommitHash == null) {
      return dag;
    }
    const dest = dag.resolve(latestSuccessor(dag, this.destination));
    const src = dag.get(headCommitHash);
    if (dest == null || src == null || dest.hash === src.hash) {
      return dag;
    }
    return dag.replaceWith([src.hash, dest.hash], (h, c) => {
      const isDest = h === dest.hash;
      const previewType = isDest
        ? CommitPreview.GOTO_DESTINATION
        : CommitPreview.GOTO_PREVIOUS_LOCATION;
      return c?.merge({isDot: isDest, previewType});
    });
  }
}
