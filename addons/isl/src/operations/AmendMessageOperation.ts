/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Dag} from '../previews';
import type {CommandArg, ExactRevset, SucceedableRevset} from '../types';

import {Operation} from './Operation';

export class AmendMessageOperation extends Operation {
  constructor(private revset: SucceedableRevset | ExactRevset, private message: string) {
    super('AmendMessageOperation');
  }

  static opName = 'Metaedit';

  getArgs() {
    const args: Array<CommandArg> = ['metaedit', '--rev', this.revset, '--message', this.message];
    return args;
  }

  optimisticDag(dag: Dag): Dag {
    const hash = this.revset.revset;
    return dag.touch(hash).replaceWith(hash, (_h, c) => {
      if (c === undefined) {
        // metaedit succeeds when we no longer see original commit
        // Note: this assumes we always restack children and never render old commit as obsolete.
        return c;
      }
      const [title] = this.message.split(/\n+/, 1);
      const description = this.message.slice(title.length);
      return {...c, title, description};
    });
  }
}
