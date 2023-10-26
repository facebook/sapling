/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ApplyPreviewsFuncType, PreviewContext} from '../previews';
import type {CommandArg, Hash} from '../types';

import {succeedableRevset} from '../types';
import {Operation} from './Operation';

export class AmendMessageOperation extends Operation {
  constructor(private hash: Hash, private message: string) {
    super('AmendMessageOperation');
  }

  static opName = 'Metaedit';

  getArgs() {
    const args: Array<CommandArg> = [
      'metaedit',
      '--rev',
      succeedableRevset(this.hash),
      '--message',
      this.message,
    ];
    return args;
  }

  makeOptimisticApplier(context: PreviewContext): ApplyPreviewsFuncType | undefined {
    const commitToMetaedit = context.treeMap.get(this.hash);
    if (commitToMetaedit == null) {
      // metaedit succeeds when we no longer see original commit
      // Note: this assumes we always restack children and never render old commit as obsolete.
      return undefined;
    }

    const [title] = this.message.split(/\n+/, 1);
    const description = this.message.slice(title.length);

    const func: ApplyPreviewsFuncType = (tree, _previewType) => {
      if (tree.info.hash === this.hash) {
        // use fake title/description on the changed commit
        return {
          info: {
            ...tree.info,
            title,
            description: description ?? '',
          },
          children: tree.children,
        };
      } else {
        return {info: tree.info, children: tree.children};
      }
    };
    return func;
  }
}
