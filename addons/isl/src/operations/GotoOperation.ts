/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ApplyPreviewsFuncType, PreviewContext} from '../previews';
import type {Hash} from '../types';

import {CommitPreview} from '../previews';
import {SucceedableRevset} from '../types';
import {Operation} from './Operation';

export class GotoOperation extends Operation {
  constructor(private destination: Hash) {
    super();
  }

  static opName = 'Goto';

  getArgs() {
    const args = ['goto', '--rev', SucceedableRevset(this.destination)];
    return args;
  }

  makeOptimisticApplier(context: PreviewContext): ApplyPreviewsFuncType | undefined {
    const headCommitHash = context.headCommit?.hash;
    if (
      headCommitHash === this.destination ||
      context.headCommit?.remoteBookmarks?.includes(this.destination)
    ) {
      // head is destination => the goto completed
      return undefined;
    }

    const func: ApplyPreviewsFuncType = (tree, _previewType) => {
      if (
        tree.info.hash === this.destination ||
        tree.info.remoteBookmarks?.includes(this.destination)
      ) {
        const modifiedInfo = {...tree.info, isHead: true};
        // this is the commit we're moving to
        return {
          info: modifiedInfo,
          children: tree.children,
          previewType: CommitPreview.GOTO_DESTINATION,
        };
      } else if (tree.info.hash === headCommitHash) {
        const modifiedInfo = {...tree.info, isHead: false};
        // this is the previous head commit, where we used to be
        return {
          info: modifiedInfo,
          children: tree.children,
          previewType: CommitPreview.GOTO_PREVIOUS_LOCATION,
        };
      } else {
        return {info: tree.info, children: tree.children};
      }
    };
    return func;
  }
}
