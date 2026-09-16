/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommandArg, ExactRevset, SucceedableRevset} from '../types';

import {Operation} from './Operation';

export type PrSubmitOptions = {
  draft?: boolean;
  updateMessage?: string;
  revision?: SucceedableRevset | ExactRevset;
  submitStack?: boolean;
  reviewers?: Array<string>;
};

export class PrSubmitOperation extends Operation {
  static opName = 'pr submit';

  constructor(private options?: PrSubmitOptions) {
    super('PrSubmitOperation');
  }

  getArgs() {
    const args: Array<CommandArg> = ['pr', 'submit', '--config', 'github.submit-to-upstream=false'];
    if (this.options?.draft) {
      args.push('--draft');
    }
    if (this.options?.updateMessage) {
      args.push('--message', this.options?.updateMessage);
    }
    if (this.options?.revision) {
      args.push('--rev', this.options.revision);
    }
    if (this.options?.submitStack) {
      args.push('--stack');
    }
    for (const reviewer of this.options?.reviewers ?? []) {
      args.push('--reviewer', reviewer);
    }
    return args;
  }
}
